use std::{collections::{BTreeMap, BTreeSet}, hash::Hash, marker::PhantomData, str::FromStr};

use arca::Path;
use serde::{Deserialize, Serialize};

use crate::{build, cache::CompositeCache, error::Error, fetchers::{fetch_locator, PackageData}, graph::{GraphCache, GraphIn, GraphOut, GraphTasks}, linker, lockfile::{Lockfile, LockfileEntry, LockfileMetadata}, content_flags::ContentFlags, primitives::{range, Descriptor, Ident, Locator, PeerRange, Range, Reference}, print_time, project::Project, resolvers::{resolve_descriptor, resolve_locator, Resolution}, semver, system, tree_resolver::{ResolutionTree, TreeResolver}, ui};


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

#[derive(Clone, Debug)]
pub struct PinnedResult {
    pub locator: Locator,
}

#[derive(Clone, Debug)]
pub struct ResolutionResult {
    pub resolution: Resolution,
    pub original_resolution: Resolution,
    pub package_data: Option<PackageData>,
}

pub trait IntoResolutionResult {
    fn into_resolution_result(self, context: &InstallContext<'_>) -> ResolutionResult;
}

#[derive(Clone, Debug)]
pub struct FetchResult {
    pub resolution: Option<Resolution>,
    pub package_data: PackageData,
}

impl FetchResult {
    pub fn new(package_data: PackageData) -> Self {
        Self {
            resolution: None,
            package_data,
        }
    }
}

impl IntoResolutionResult for FetchResult {
    fn into_resolution_result(self, context: &InstallContext<'_>) -> ResolutionResult {
        let mut resolution = self.resolution
            .expect("Expected this fetch result to contain a resolution record to be convertible into a resolution result");

        let original_resolution = resolution.clone();

        let (dependencies, peer_dependencies)
            = normalize_resolutions(context, &resolution);

        resolution.dependencies = dependencies;
        resolution.peer_dependencies = peer_dependencies;

        ResolutionResult {
            resolution,
            original_resolution,
            package_data: Some(self.package_data),
        }
    }
}

#[derive(Clone, Debug)]
pub enum InstallOpResult {
    Pinned(PinnedResult),
    Resolved(ResolutionResult),
    Fetched(FetchResult),
}

impl InstallOpResult {
    pub fn into_resolved(self) -> ResolutionResult {
        match self {
            InstallOpResult::Resolved(resolution) => resolution,
            _ => panic!("Expected a resolved result"),
        }
    }

    pub fn into_fetched(self) -> FetchResult {
        match self {
            InstallOpResult::Fetched(fetch) => fetch,
            _ => panic!("Expected a fetched result"),
        }
    }

    pub fn as_resolved(&self) -> &ResolutionResult {
        match self {
            InstallOpResult::Resolved(resolution) => resolution,
            _ => panic!("Expected a resolved result; got {:?}", self),
        }
    }

    pub fn as_fetched(&self) -> &FetchResult {
        match self {
            InstallOpResult::Fetched(fetch) => fetch,
            _ => panic!("Expected a fetched result"),
        }
    }

    pub fn as_resolved_locator(&self) -> &Locator {
        match self {
            InstallOpResult::Resolved(params) => &params.resolution.locator,
            InstallOpResult::Pinned(params) => &params.locator,
            _ => panic!("Expected a resolved locator; got {:?}", self),
        }
    }
}

impl<'a> GraphOut<InstallContext<'a>, InstallOp<'a>> for InstallOpResult {
    fn graph_follow_ups(&self, _ctx: &InstallContext<'a>) -> Vec<InstallOp<'a>> {
        match self {
            InstallOpResult::Pinned(PinnedResult {locator, ..}) => {
                vec![InstallOp::Refresh {
                    locator: locator.clone(),
                }]
            },

            InstallOpResult::Resolved(ResolutionResult {resolution, ..}) => {
                let mut follow_ups = vec![InstallOp::Fetch {
                    locator: resolution.locator.clone(),
                }];

                let transitive_dependencies = resolution.dependencies
                    .values()
                    .cloned()
                    .map(|dependency| InstallOp::Resolve {descriptor: dependency});

                follow_ups.extend(transitive_dependencies);
                follow_ups
            },

            InstallOpResult::Fetched(FetchResult {..}) => {
                vec![]
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum InstallOp<'a> {
    #[allow(dead_code)]
    Phantom(PhantomData<&'a ()>),

    Refresh {
        locator: Locator,
    },

    Resolve {
        descriptor: Descriptor,
    },

    Fetch {
        locator: Locator,
    },
}

impl<'a> GraphIn<'a, InstallContext<'a>, InstallOpResult, Error> for InstallOp<'a> {
    fn graph_dependencies(&self, _ctx: &InstallContext<'a>, resolved_dependencies: &Vec<&InstallOpResult>) -> Vec<Self> {
        let mut dependencies = vec![];
        let mut resolved_it = resolved_dependencies.iter();

        match self {
            InstallOp::Phantom(_) =>
                unreachable!("PhantomData should never be instantiated"),

            InstallOp::Refresh {locator} => {
                if let Some(parent) = &locator.parent {
                    dependencies.push(InstallOp::Fetch {locator: parent.as_ref().clone()});
                    resolved_it.next();
                }

                if let Reference::Patch(params) = &locator.reference {
                    dependencies.push(InstallOp::Fetch {locator: params.inner.as_ref().0.clone()});
                }
            },

            InstallOp::Resolve {descriptor} => {
                if let Some(parent) = &descriptor.parent {
                    dependencies.push(InstallOp::Fetch {locator: parent.clone()});
                    resolved_it.next();
                }

                if let Range::Patch(params) = &descriptor.range {
                    assert!(descriptor.parent.is_some(), "Expected a parent to be set for a patch resolution");

                    let mut inner_descriptor = params.inner.to_owned().0.clone();

                    if inner_descriptor.range.must_bind() {
                        inner_descriptor.parent = descriptor.parent.clone();
                    }

                    dependencies.push(InstallOp::Resolve {descriptor: inner_descriptor});
                    let patch_resolution = resolved_it.next();

                    if let Some(result) = patch_resolution {
                        let patch_resolved_locator = result.as_resolved_locator();
                        dependencies.push(InstallOp::Fetch {locator: patch_resolved_locator.clone()});
                    }
                }
            },

            InstallOp::Fetch {locator} => {
                if let Some(parent) = &locator.parent {
                    dependencies.push(InstallOp::Fetch {locator: parent.as_ref().clone()});
                }

                if let Reference::Patch(params) = &locator.reference {
                    dependencies.push(InstallOp::Fetch {locator: params.inner.to_owned().0.clone()});
                }
            },
        }

        dependencies
    }

    async fn graph_run(self, context: InstallContext<'a>, dependencies: Vec<InstallOpResult>) -> Result<InstallOpResult, Error> {
        let timeout = std::time::Duration::from_secs(600);
        match self {
            InstallOp::Phantom(_) =>
                unreachable!("PhantomData should never be instantiated"),

            InstallOp::Refresh {locator} => {
                let future = tokio::time::timeout(
                    timeout,
                    resolve_locator(context.clone(), locator.clone(), dependencies)
                ).await.map_err(|_| Error::TaskTimeout)?;

                Ok(InstallOpResult::Resolved(future?))
            },

            InstallOp::Resolve {descriptor} => {
                let future = tokio::time::timeout(
                    timeout,
                    resolve_descriptor(context.clone(), descriptor.clone(), dependencies)
                ).await.map_err(|_| Error::TaskTimeout)?;

                Ok(InstallOpResult::Resolved(future?))
            },

            InstallOp::Fetch {locator} => {
                let future = tokio::time::timeout(
                    timeout,
                    fetch_locator(context.clone(), &locator.clone(), false, dependencies)
                ).await.map_err(|_| Error::TaskTimeout)?;

                Ok(InstallOpResult::Fetched(future?))
            },

        }
    }
}

struct InstallCache {
    pub lockfile: Lockfile,
}

impl InstallCache {
    pub fn new(lockfile: Lockfile) -> Self {
        Self {
            lockfile,
        }
    }
}

impl<'a> GraphCache<InstallContext<'a>, InstallOp<'a>, InstallOpResult> for InstallCache {
    fn graph_cache(&self, ctx: &InstallContext<'a>, op: &InstallOp) -> Option<InstallOpResult> {
        if let InstallOp::Resolve {descriptor} = op {
            if let Some(locator) = self.lockfile.resolutions.get(descriptor) {
                if self.lockfile.metadata.version != LockfileMetadata::new().version {
                    return Some(InstallOpResult::Pinned(PinnedResult {
                        locator: locator.clone(),
                    }));
                }

                let entry = self.lockfile.entries.get(locator)
                    .unwrap_or_else(|| panic!("Expected a matching resolution to be found in the lockfile for any resolved locator; not found for {}.", locator));

                return Some(InstallOpResult::Resolved(entry.resolution.clone().into_resolution_result(ctx)));
            }
        }

        None
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct InstallState {
    pub lockfile: Lockfile,
    pub resolution_tree: ResolutionTree,
    pub normalized_resolutions: BTreeMap<Locator, Resolution>,
    pub packages_by_location: BTreeMap<Path, Locator>,
    pub locations_by_package: BTreeMap<Locator, Path>,
    pub optional_packages: BTreeSet<Locator>,
    pub disabled_locators: BTreeSet<Locator>,
    pub conditional_locators: BTreeSet<Locator>,
    pub package_flags: BTreeMap<Locator, ContentFlags>,
}

#[derive(Clone, Default)]
pub struct Install {
    pub package_data: BTreeMap<Locator, PackageData>,
    pub install_state: InstallState,
}

impl Install {
    pub async fn finalize(mut self, project: &mut Project) -> Result<(), Error> {
        print_time!("Before link");

        let build = linker::link_project(project, &mut self)
            .await?;

        print_time!("Before build");

        project
            .attach_install_state(self.install_state)?;

        let result = build::BuildManager::new(build)
            .run(project).await?;

        print_time!("Done");

        if !result.build_errors.is_empty() {
            println!("Build errors: {:?}", result.build_errors);
            return Err(Error::Unsupported);
        }

        Ok(())
    }
}

pub struct InstallManager<'a> {
    description: system::Description,
    initial_lockfile: Lockfile,
    roots: Vec<Descriptor>,
    context: InstallContext<'a>,
    result: Install,
}

impl<'a> Default for InstallManager<'a> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> InstallManager<'a> {
    pub fn new() -> Self {
        InstallManager {
            description: system::Description::from_current(),
            initial_lockfile: Lockfile::new(),
            roots: vec![],
            context: InstallContext::default(),
            result: Install::default(),
        }
    }

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

    pub async fn resolve_and_fetch(mut self) -> Result<Install, Error> {
        let cache = InstallCache::new(self.initial_lockfile.clone());

        let mut graph
            = GraphTasks::new(self.context.clone(), cache);

        for descriptor in self.roots.clone() {
            graph.register(InstallOp::Resolve {
                descriptor,
            });
        }

        let spinner = ui::spinner::Spinner::open();

        let graph_run
            = graph.run().await;

        spinner.close();

        if let Some(error) = graph_run.get_failed() {
            println!("Graph errors: {:#?}", error);
        }

        for entry in graph_run.unwrap() {
            match entry {
                (InstallOp::Resolve {descriptor, ..}, InstallOpResult::Pinned(PinnedResult {locator})) => {
                    self.record_descriptor(descriptor, locator);
                },

                (InstallOp::Refresh {..}, InstallOpResult::Resolved(ResolutionResult {resolution, original_resolution, package_data})) => {
                    self.record_resolution(resolution, original_resolution, package_data)?;
                },

                (InstallOp::Resolve {descriptor, ..}, InstallOpResult::Resolved(ResolutionResult {resolution, original_resolution, package_data})) => {
                    self.record_descriptor(descriptor, resolution.locator.clone());
                    self.record_resolution(resolution, original_resolution, package_data)?;
                },

                (InstallOp::Fetch {locator, ..}, InstallOpResult::Fetched(FetchResult {package_data, ..})) => {
                    self.record_fetch(locator, package_data)?;
                },

                _ => panic!("Unsupported install result ({:?})", entry),
            }
        }

        self.result.install_state.resolution_tree = TreeResolver::default()
            .with_resolutions(&self.result.install_state.lockfile.resolutions, &self.result.install_state.normalized_resolutions)
            .with_roots(self.roots.clone())
            .run();

        Ok(self.result)
    }

    fn record_resolution(&mut self, resolution: Resolution, original_resolution: Resolution, package_data: Option<PackageData>) -> Result<(), Error> {
        self.result.install_state.normalized_resolutions.insert(resolution.locator.clone(), resolution.clone());

        self.result.install_state.lockfile.entries.insert(resolution.locator.clone(), LockfileEntry {
            checksum: None,
            resolution: original_resolution.clone(),
        });

        if resolution.requirements.is_conditional() {
            self.result.install_state.conditional_locators.insert(resolution.locator.clone());

            if !resolution.requirements.validate(&self.description) {
                self.result.install_state.disabled_locators.insert(resolution.locator.clone());
            }
        }

        if let Some(package_data) = package_data {
            self.record_fetch(resolution.locator.clone(), package_data)?;
        }

        Ok(())
    }

    fn record_descriptor(&mut self, descriptor: Descriptor, locator: Locator) {
        self.result.install_state.lockfile.resolutions.insert(descriptor, locator);
    }

    fn record_fetch(&mut self, locator: Locator, package_data: PackageData) -> Result<(), Error> {
        let project = self.context.project
            .expect("The project is required to record a fetch result");

        let content_flags = match project.install_state.as_ref().and_then(|s| s.package_flags.get(&locator)) {
            Some(flags) => flags.clone(),
            None => ContentFlags::extract(&locator, &package_data)?,
        };

        self.result.install_state.package_flags.insert(locator.clone(), content_flags);
        self.result.package_data.insert(locator, package_data);

        Ok(())
    }
}

pub fn normalize_resolutions(context: &InstallContext<'_>, resolution: &Resolution) -> (BTreeMap<Ident, Descriptor>, BTreeMap<Ident, PeerRange>) {
    let root_workspace = context.project
        .expect("The project is required to bind a parent to a descriptor")
        .root_workspace();

    let mut dependencies = resolution.dependencies.clone();
    let mut peer_dependencies = resolution.peer_dependencies.clone();

    // Some protocols need to know about the package that declares the
    // dependency (for example the `portal:` protocol, which always points
    // to a location relative to the parent package. We mutate the
    // descriptors for these protocols to "bind" them to a particular
    // parent descriptor. In effect, it means we're creating a unique
    // version of the package, which will be resolved / fetched
    // independently from any other.
    //
    for descriptor in dependencies.values_mut() {
        let possible_resolution_overrides = context.project
            .and_then(|project| project.resolution_overrides(&descriptor.ident));

        let resolution_override = possible_resolution_overrides
            .and_then(|overrides| {
                overrides.iter().find_map(|(rule, range)| {
                    rule.apply(&resolution.locator, &resolution.version, descriptor, range)
                })
            });

        if let Some(replacement_range) = resolution_override {
            descriptor.range = replacement_range;

            if descriptor.range.must_bind() {
                descriptor.parent = Some(root_workspace.locator());
            }
        } else if descriptor.range.must_bind() {
            descriptor.parent = Some(resolution.locator.clone());
        }

        match &descriptor.range {
            Range::AnonymousSemver(params)
                => descriptor.range = range::RegistrySemverRange {ident: None, range: params.range.clone()}.into(),

            Range::AnonymousTag(params)
                => descriptor.range = range::RegistryTagRange {ident: None, tag: params.tag.clone()}.into(),

            _ => {},
        };
    }

    for name in peer_dependencies.keys().filter(|ident| ident.scope() != Some("@types")).cloned().collect::<Vec<_>>() {
        peer_dependencies.entry(name.type_ident())
            .or_insert(range::SemverPeerRange {range: semver::Range::from_str("*").unwrap()}.into());
    }

    (dependencies, peer_dependencies)
}
