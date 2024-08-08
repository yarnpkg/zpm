use std::{collections::{HashMap, HashSet}, convert::Infallible, marker::PhantomData, str::FromStr};

use arca::Path;
use futures::{future::BoxFuture, stream::FuturesUnordered, StreamExt};
use serde::{Deserialize, Serialize};

use crate::{build, cache::CompositeCache, error::Error, fetcher::{fetch, PackageData}, linker, lockfile::{Lockfile, LockfileEntry}, primitives::{Descriptor, Locator, PeerRange}, project::Project, resolver::{resolve, Resolution, ResolveResult}, semver, tree_resolver::{ResolutionTree, TreeResolver}};

#[derive(Clone, Default)]
pub struct InstallContext<'a> {
    pub package_cache: Option<&'a CompositeCache>,
    pub project: Option<&'a Project>,
}

impl<'a> InstallContext<'a> {
    pub fn with_package_cache(mut self, package_cache: Option<&'a CompositeCache>) -> Self {
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
        package_data: Option<PackageData>,
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

            InstallOp::Resolve {descriptor, parent_data} => {
                match resolve(context, descriptor.clone(), parent_data).await {
                    Err(error) => InstallOpResult::ResolutionFailed {
                        descriptor,
                        error,
                    },

                    Ok(ResolveResult {resolution, package_data}) => InstallOpResult::Resolved {
                        descriptor,
                        resolution,
                        package_data,
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

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct InstallState {
    pub lockfile: Lockfile,
    pub resolution_tree: ResolutionTree,
    pub packages_by_location: HashMap<Path, Locator>,
    pub locations_by_package: HashMap<Locator, Path>,
}

#[derive(Clone, Default)]
pub struct Install {
    pub package_data: HashMap<Locator, PackageData>,
    pub install_state: InstallState,
}

impl Install {
    pub async fn finalize(mut self, project: &mut Project) -> Result<(), Error> {
        let build = linker::link_project(project, &mut self)
            .await?;

        project
            .attach_install_state(self.install_state)?;

        build::BuildManager::new(build)
            .run(project).await?;

        Ok(())
    }
}

#[derive(Default)]
pub struct InstallManager<'a> {
    initial_lockfile: Lockfile,
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
        self.initial_lockfile = lockfile;
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
            if let Some(locator) = self.initial_lockfile.resolutions.remove(&descriptor) {
                let entry = self.initial_lockfile.entries.get(&locator)
                    .expect("Expected a matching resolution to be found in the lockfile for any resolved locator.");

                self.record_resolution(descriptor, entry.resolution.clone(), None);
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

    fn record_resolution(&mut self, descriptor: Descriptor, mut resolution: Resolution, package_data: Option<PackageData>) {
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

        for name in resolution.peer_dependencies.keys().cloned().collect::<Vec<_>>() {
            resolution.peer_dependencies.entry(name.type_ident())
                .or_insert(PeerRange::Semver(semver::Range::from_str("*").unwrap()));
        }

        self.result.install_state.lockfile.resolutions.insert(descriptor, resolution.locator.clone());
        self.result.install_state.lockfile.entries.insert(resolution.locator.clone(), LockfileEntry {
            checksum: None,
            resolution: resolution.clone(),
        });

        if let Some(package_data) = package_data {
            self.record_fetch(resolution.locator.clone(), package_data.clone());
        } else {
            self.ops.push(InstallOp::Fetch {
                locator: resolution.locator.clone(),
                parent_data,
            });
        }
    }

    fn record_fetch(&mut self, locator: Locator, package_data: PackageData) {
        self.result.package_data.insert(locator.clone(), package_data.clone());

        if let Some(deferred) = self.deferred.remove(&locator) {
            for descriptor in deferred {
                self.seen.remove(&descriptor);
                self.schedule(descriptor);
            }
        }
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

    pub async fn resolve_and_fetch(mut self) -> Result<Install, Error> {
        for descriptor in self.roots.clone() {
            self.schedule(descriptor);
        }

        self.trigger();

        while let Some(res) = self.running.next().await {
            match res {
                InstallOpResult::Resolved {descriptor, resolution, package_data} => {
                    self.record_resolution(descriptor, resolution, package_data);
                }

                InstallOpResult::Fetched {locator, package_data} => {
                    self.record_fetch(locator, package_data);
                }

                InstallOpResult::FetchFailed {locator, error} => {
                    println!("{}: {:?}", locator, error);
                }

                InstallOpResult::ResolutionFailed {descriptor, error} => {
                    println!("{}: {:?}", descriptor, error);
                }
            }

            self.trigger();
        }

        if !self.deferred.is_empty() {
            panic!("Some deferred descriptors were not resolved");
        }

        self.result.install_state.resolution_tree = TreeResolver::default()
            .with_lockfile(self.result.install_state.lockfile.clone())
            .with_roots(self.roots.clone())
            .run();

        Ok(self.result)
    }
}
