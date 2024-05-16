use std::{collections::{HashMap, HashSet}, convert::Infallible, marker::PhantomData};

use futures::{future::BoxFuture, stream::FuturesUnordered, StreamExt};

use crate::{cache::DiskCache, error::Error, fetcher::{fetch, PackageData}, lockfile::{Lockfile, LockfileEntry}, primitives::{Descriptor, Locator}, project::Project, resolver::{resolve, Resolution}, tree_resolver::{ResolutionTree, TreeResolver}};

#[derive(Clone, Default)]
pub struct InstallContext<'a> {
    pub package_cache: Option<&'a DiskCache>,
    pub project: Option<&'a Project>,
}

impl<'a> InstallContext<'a> {
    pub fn with_package_cache(mut self, package_cache: Option<&'a DiskCache>) -> Self {
        self.package_cache = package_cache;
        self
    }

    pub fn with_project(mut self, project: Option<&'a Project>) -> Self {
        self.project = project;
        self
    }
}

#[derive(Debug)]
enum InstallOpResult {
    Resolved {
        descriptor: Descriptor,
        resolution: Resolution,
    },

    ResolutionFailed {
        descriptor: Descriptor,
        error: Error,
    },

    Fetched {
        locator: Locator,
        package_data: PackageData,
    },

    FetchFailed {
        locator: Locator,
        error: Error,
    },
}

#[derive(Debug)]
enum InstallOp<'a> {
    Phantom(Infallible, PhantomData<&'a ()>),

    Resolve {
        descriptor: Descriptor,
        parent_data: Option<PackageData>,
    },

    Fetch {
        locator: Locator,
        parent_data: Option<PackageData>,
    },
}

impl<'a> InstallOp<'a> {
    pub async fn run(self, context: InstallContext<'a>) -> InstallOpResult {
        match self {
            InstallOp::Phantom(_, _) =>
                unreachable!("PhantomData should never be instantiated"),

            InstallOp::Resolve {descriptor, ..} => {
                match resolve(context, descriptor.clone()).await {
                    Err(error) => InstallOpResult::ResolutionFailed {
                        descriptor,
                        error,
                    },

                    Ok(resolution) => InstallOpResult::Resolved {
                        descriptor,
                        resolution,
                    },
                }
            },

            InstallOp::Fetch {locator, parent_data} => {
                match fetch(context, &locator.clone(), parent_data).await {
                    Err(error) => InstallOpResult::FetchFailed {
                        locator,
                        error,
                    },

                    Ok(package_data) => InstallOpResult::Fetched {
                        locator,
                        package_data,
                    },
                }
            },
        }
    }
}

#[derive(Clone, Default)]
pub struct Install {
    pub lockfile: Lockfile,
    pub package_data: HashMap<Locator, PackageData>,
    pub resolution_tree: ResolutionTree,
}

impl Install {
    pub fn lockfile_string() {

    }
}

#[derive(Default)]
pub struct InstallManager<'a> {
    lockfile: Lockfile,
    roots: Vec<Descriptor>,
    context: InstallContext<'a>,
    result: Install,
    ops: Vec<InstallOp<'a>>,
    deferred: HashMap<Locator, Vec<Descriptor>>,
    running: FuturesUnordered<BoxFuture<'a, InstallOpResult>>,
    seen: HashSet<Descriptor>,
}

impl<'a> InstallManager<'a> {
    pub fn with_context(mut self, context: InstallContext<'a>) -> Self {
        self.context = context;
        self
    }

    pub fn with_lockfile(mut self, lockfile: Lockfile) -> Self {
        self.lockfile = lockfile;
        self
    }

    pub fn with_roots(mut self, roots: Vec<Descriptor>) -> Self {
        self.roots = roots;
        self
    }

    pub fn with_roots_iter<T: Iterator<Item = Descriptor>>(self, it: T) -> Self {
        self.with_roots(it.collect())
    }

    fn schedule(&mut self, descriptor: Descriptor) {
        if !self.seen.insert(descriptor.clone()) {
            return;
        }

        if descriptor.parent.is_none() {
            if let Some(locator) = self.lockfile.resolutions.remove(&descriptor) {
                let entry = self.lockfile.entries.get(&locator)
                    .expect("Expected a matching resolution to be found in the lockfile for any resolved locator.");

                self.record(descriptor, entry.resolution.clone());
                return;
            }
        }

        if let Some(parent) = &descriptor.parent {
            if let Some(package_data) = self.result.package_data.get(parent) {
                self.ops.push(InstallOp::Resolve {descriptor, parent_data: Some(package_data.clone())});
            } else {
                self.deferred.entry(parent.clone()).or_insert(vec![]).push(descriptor);
            }
        } else {
            self.ops.push(InstallOp::Resolve {descriptor, parent_data: None});
        }
    }

    fn record(&mut self, descriptor: Descriptor, mut resolution: Resolution) {
        for descriptor in resolution.dependencies.values_mut() {
            if descriptor.range.must_bind() {
                descriptor.parent = Some(resolution.locator.clone());
            }
        }

        let transitive_dependencies = resolution.dependencies
            .values()
            .cloned();

        for descriptor in transitive_dependencies {
            self.schedule(descriptor);
        }

        let parent_data = match &descriptor.parent {
            Some(parent) => Some(self.result.package_data.get(parent).expect("Parent data not found").clone()),
            None => None,
        };

        self.result.lockfile.resolutions.insert(descriptor, resolution.locator.clone());
        self.result.lockfile.entries.insert(resolution.locator.clone(), LockfileEntry {
            checksum: None,
            resolution: resolution.clone(),
        });

        self.ops.push(InstallOp::Fetch {
            locator: resolution.locator.clone(),
            parent_data,
        });
    }

    fn trigger(&mut self) {
        while self.running.len() < 100 {
            if let Some(op) = self.ops.pop() {
                self.running.push(Box::pin(op.run(self.context.clone())));
            } else {
                break;
            }
        }
    }

    pub async fn run(mut self) -> Result<Install, Error> {
        for descriptor in self.roots.clone() {
            self.schedule(descriptor);
        }

        self.trigger();

        while let Some(res) = self.running.next().await {
            match res {
                InstallOpResult::Resolved {descriptor, resolution} => {
                    self.record(descriptor, resolution);
                }

                InstallOpResult::Fetched {locator, package_data} => {
                    self.result.package_data.insert(locator.clone(), package_data);

                    if let Some(deferred_descriptors) = self.deferred.remove(&locator) {
                        for descriptor in deferred_descriptors {
                            self.seen.remove(&descriptor);
                            self.schedule(descriptor);
                        }
                    }
                }

                _ => {}
            }

            self.trigger();
        }

        if !self.deferred.is_empty() {
            panic!("Some deferred descriptors were not resolved");
        }

        self.result.resolution_tree = TreeResolver::default()
            .with_lockfile(self.result.lockfile.clone())
            .with_roots(self.roots.clone())
            .run();

        Ok(self.result)
    }
}
