use std::{collections::{BTreeMap, BTreeSet, HashSet}, hash::Hash, marker::PhantomData};

use rayon::iter::{IntoParallelIterator, ParallelIterator};
use sha2::Digest;
use zpm_utils::{Path, ToHumanString};
use bincode::{Decode, Encode};
use serde::{Deserialize, Serialize};
use zpm_utils::{FromFileString, ToFileString};

use crate::{build, cache::CompositeCache, content_flags::ContentFlags, error::Error, fetchers::{fetch_locator, patch::has_builtin_patch, try_fetch_locator_sync, PackageData, SyncFetchAttempt}, graph::{GraphCache, GraphIn, GraphOut, GraphTasks}, hash::Sha256, linker, lockfile::{Lockfile, LockfileEntry, LockfileMetadata}, primitives::{range, Descriptor, Ident, Locator, PeerRange, Range, Reference}, project::Project, report::{async_section, with_context_result, ReportContext}, resolvers::{resolve_descriptor, resolve_locator, try_resolve_descriptor_sync, validate_resolution, Resolution, SyncResolutionAttempt}, serialize::UrlEncoded, system, tree_resolver::{ResolutionTree, TreeResolver}};


#[derive(Clone, Default)]
pub struct InstallContext<'a> {
    pub package_cache: Option<&'a CompositeCache>,
    pub project: Option<&'a Project>,
    pub check_checksums: bool,
    pub check_resolutions: bool,
    pub refresh_lockfile: bool,
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

    pub fn set_check_checksums(mut self, check_checksums: bool) -> Self {
        self.check_checksums = check_checksums;
        self
    }

    pub fn set_check_resolutions(mut self, check_resolutions: bool) -> Self {
        self.check_resolutions = check_resolutions;
        self
    }

    pub fn set_refresh_lockfile(mut self, refresh_lockfile: bool) -> Self {
        self.refresh_lockfile = refresh_lockfile;
        self
    }
}

#[derive(Clone, Debug)]
pub struct PinnedResult {
    pub locator: Locator,
}

#[derive(Clone, Debug)]
pub struct ValidatedResult {
    pub success: bool,
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
    Validated,
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
            InstallOpResult::Validated => {
                vec![]
            },

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

    Validate {
        descriptor: Descriptor,
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
    fn graph_dependencies(&self, ctx: &InstallContext<'a>, resolved_dependencies: &[&InstallOpResult]) -> Vec<Self> {
        let mut dependencies = vec![];
        let mut resolved_it = resolved_dependencies.iter();

        match self {
            InstallOp::Phantom(_) =>
                unreachable!("PhantomData should never be instantiated"),

            InstallOp::Validate {descriptor, ..} => {
                InstallOp::Resolve {
                    descriptor: descriptor.clone(),
                }.graph_dependencies(ctx, resolved_dependencies);
            },

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

            InstallOp::Validate {descriptor, locator} => {
                with_context_result(ReportContext::Descriptor(descriptor.clone()), async {
                    tokio::time::timeout(
                        timeout,
                        validate_resolution(context.clone(), descriptor.clone(), locator.clone(), dependencies)
                    ).await.map_err(|_| Error::TaskTimeout)??;

                    Ok(InstallOpResult::Validated)
                }).await
            },

            InstallOp::Refresh {locator} => {
                with_context_result(ReportContext::Locator(locator.clone()), async {
                    let future = tokio::time::timeout(
                        timeout,
                        resolve_locator(context.clone(), locator.clone(), dependencies)
                    ).await.map_err(|_| Error::TaskTimeout)?;

                    Ok(InstallOpResult::Resolved(future?))
                }).await
            },

            InstallOp::Resolve {descriptor} => {
                let dependencies = match try_resolve_descriptor_sync(context.clone(), descriptor.clone(), dependencies)? {
                    SyncResolutionAttempt::Success(result) => return Ok(InstallOpResult::Resolved(result)),
                    SyncResolutionAttempt::Failure(dependencies) => dependencies,
                };

                with_context_result(ReportContext::Descriptor(descriptor.clone()), async {
                    let future = tokio::time::timeout(
                        timeout,
                        resolve_descriptor(context.clone(), descriptor.clone(), dependencies)
                    ).await.map_err(|_| Error::TaskTimeout)?;

                    Ok(InstallOpResult::Resolved(future?))
                }).await
            },

            InstallOp::Fetch {locator} => {
                let dependencies = match try_fetch_locator_sync(context.clone(), &locator, false, dependencies)? {
                    SyncFetchAttempt::Success(result) => return Ok(InstallOpResult::Fetched(result)),
                    SyncFetchAttempt::Failure(dependencies) => dependencies,
                };

                with_context_result(ReportContext::Locator(locator.clone()), async {
                    let future = tokio::time::timeout(
                        timeout,
                        fetch_locator(context.clone(), &locator.clone(), false, dependencies)
                    ).await.map_err(|_| Error::TaskTimeout)?;

                    Ok(InstallOpResult::Fetched(future?))
                }).await
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
            if !descriptor.range.is_transient_resolution() {
                if let Some(locator) = self.lockfile.resolutions.get(descriptor) {
                    if self.lockfile.metadata.version != LockfileMetadata::new().version || ctx.refresh_lockfile {
                        return Some(InstallOpResult::Pinned(PinnedResult {
                            locator: locator.clone(),
                        }));
                    }

                    let entry = self.lockfile.entries.get(locator)
                        .unwrap_or_else(|| panic!("Expected a matching resolution to be found in the lockfile for any resolved locator; not found for {}.", locator.to_print_string()));

                    return Some(InstallOpResult::Resolved(entry.resolution.clone().into_resolution_result(ctx)));
                }
            }
        }

        None
    }
}

#[derive(Clone, Debug, Encode, Decode, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct InstallState {
    pub last_installed_at: u128,
    pub resolution_tree: ResolutionTree,
    pub normalized_resolutions: BTreeMap<Locator, Resolution>,
    pub packages_by_location: BTreeMap<Path, Locator>,
    pub locations_by_package: BTreeMap<Locator, Path>,
    pub optional_packages: BTreeSet<Locator>,
    pub disabled_locators: BTreeSet<Locator>,
    pub conditional_locators: BTreeSet<Locator>,
}

impl InstallState {
    pub fn locator_tree_hash(&self, root: &Locator) -> String {
        let mut hasher
            = sha2::Sha256::new();

        let mut seen
            = HashSet::new();
        let mut queue
            = vec![root];

        while let Some(locator) = queue.pop() {
            if seen.insert(locator) {
                hasher.update(locator.to_file_string())
            }
        }

        format!("{:064x}", hasher.finalize())
    }
}

#[derive(Clone, Default)]
pub struct Install {
    pub lockfile: Lockfile,
    pub lockfile_changed: bool,
    pub package_data: BTreeMap<Locator, PackageData>,
    pub install_state: InstallState,
}

impl Install {
    pub async fn finalize(mut self, project: &mut Project) -> Result<(), Error> {
        self.install_state.last_installed_at = project.last_changed_at;

        let link_future
            = linker::link_project(project, &mut self);

        let build_requests
            = async_section("Linking the project", link_future).await?;

        project.attach_install_state(self.install_state)?;
        project.write_lockfile(&self.lockfile)?;

        if !build_requests.entries.is_empty() {
            let build_future
                = build::BuildManager::new(build_requests).run(project);

            let build_result
                = async_section("Building the project", build_future).await?;

            if !build_result.build_errors.is_empty() {
                return Err(Error::SilentError);
            }
        }

        let ignore_path
            = project.ignore_path();

        if ignore_path.fs_exists() {
            ignore_path
                .with_join_str(".gitignore")
                .fs_change("*", false)?;
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

impl Default for InstallManager<'_> {
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

        let graph_run
            = async_section("Installing packages", graph.run()).await;

        let installed_entries = graph_run
            .ok_or(Error::SilentError)?;

        for entry in installed_entries {
            match entry {
                (InstallOp::Resolve {..}, InstallOpResult::Validated) => {
                },

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
                    self.record_fetch(locator, package_data);
                },

                _ => panic!("Unsupported install result ({:?})", entry),
            }
        }

        let missing_checksums = self.result.lockfile.entries.values()
            .filter(|entry| {
                let previous_entry
                    = self.initial_lockfile.entries.get(&entry.resolution.locator);

                let has_checksum
                    = previous_entry.map_or(false, |s| s.checksum.is_some());

                !has_checksum
            })
            .flat_map(|entry| {
                let package_data = self.result.package_data.get(&entry.resolution.locator)
                    .unwrap_or_else(|| panic!("Expected a matching package data to be found for any fetched locator; not found for {}.", entry.resolution.locator.to_file_string()));

                let PackageData::Zip {archive_path, ..} = package_data else {
                    return None;
                };

                Some((entry.resolution.locator.clone(), archive_path))
            })
            .collect::<Vec<_>>();

        let late_checksums = missing_checksums.into_par_iter()
            .map(|(locator, archive_path)| -> Result<_, Error> {
                let archive_data = archive_path
                    .fs_read_prealloc()?;

                let checksum
                    = Sha256::from_data(&archive_data);

                Ok((locator, checksum))
            })
            .collect::<Result<BTreeMap<_, _>, Error>>()?;

        for entry in self.result.lockfile.entries.values_mut() {
            let package_data = self.result.package_data.get(&entry.resolution.locator)
                .unwrap_or_else(|| panic!("Expected a matching package data to be found for any fetched locator; not found for {}.", entry.resolution.locator.to_file_string()));

            let previous_entry = self.initial_lockfile.entries.get(&entry.resolution.locator);
            let previous_checksum = previous_entry.and_then(|s| s.checksum.as_ref());
            let previous_flags = previous_entry.map(|s| &s.flags);

            let checksum = package_data.checksum()
                .or_else(|| previous_checksum.cloned())
                .or_else(|| late_checksums.get(&entry.resolution.locator).cloned());

            if self.context.check_checksums {
                if let Some(previous_checksum) = previous_checksum {
                    if checksum.as_ref() != Some(previous_checksum) {
                        if let PackageData::Zip {archive_path, ..} = package_data {
                            if let Some(project) = &self.context.project {
                                let quarantine_path = project.ignore_path()
                                    .with_join_str("quarantine")
                                    .with_join_str(entry.resolution.locator.slug())
                                    .with_ext("zip");

                                let data = archive_path
                                    .fs_read_prealloc()?;

                                quarantine_path
                                    .fs_create_parent()?
                                    .fs_write(&data)?;
                            }

                            return Err(Error::ChecksumMismatch(entry.resolution.locator.clone()));
                        }
                    }
                }
            }

            let content_flags = match previous_flags {
                Some(flags) => flags.clone(),
                None => ContentFlags::extract(&entry.resolution.locator, &package_data)?,
            };

            entry.checksum = checksum;
            entry.flags = content_flags;
        }

        self.result.install_state.resolution_tree = TreeResolver::default()
            .with_resolutions(&self.result.lockfile.resolutions, &self.result.install_state.normalized_resolutions)
            .with_roots(self.roots.clone())
            .run();

        self.result.lockfile_changed = self.result.lockfile != self.initial_lockfile;

        if let Some(cache) = &self.context.package_cache {
            cache.clean().await?;
        }

        Ok(self.result)
    }

    fn record_resolution(&mut self, resolution: Resolution, original_resolution: Resolution, package_data: Option<PackageData>) -> Result<(), Error> {
        self.result.install_state.normalized_resolutions.insert(resolution.locator.clone(), resolution.clone());

        self.result.lockfile.entries.insert(resolution.locator.clone(), LockfileEntry {
            checksum: None,
            resolution: original_resolution,
            flags: ContentFlags::default(),
        });

        if resolution.requirements.is_conditional() {
            self.result.install_state.conditional_locators.insert(resolution.locator.clone());

            if !resolution.requirements.validate(&self.description) {
                self.result.install_state.disabled_locators.insert(resolution.locator.clone());
            }
        }

        if let Some(package_data) = package_data {
            self.record_fetch(resolution.locator, package_data);
        }

        Ok(())
    }

    fn record_descriptor(&mut self, descriptor: Descriptor, locator: Locator) {
        self.result.lockfile.resolutions.insert(descriptor, locator);
    }

    fn record_fetch(&mut self, locator: Locator, package_data: PackageData) {
        self.result.package_data.insert(locator, package_data);
    }
}

fn normalize_resolution(context: &InstallContext<'_>, descriptor: &mut Descriptor, resolution: &Resolution, apply_overrides: bool) -> () {
    if apply_overrides {
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
                let root_workspace = context.project
                    .expect("The project is required to bind a parent to a descriptor")
                    .root_workspace();
        
                descriptor.parent = Some(root_workspace.locator());
            }
        } else if descriptor.range.must_bind() {
            descriptor.parent = Some(resolution.locator.clone());
        }
    }

    match &mut descriptor.range {
        Range::Patch(params) => {
            normalize_resolution(context, &mut params.inner.as_mut().0, resolution, false);
        },

        Range::AnonymousSemver(params)
            => descriptor.range = range::RegistrySemverRange {ident: None, range: params.range.clone()}.into(),

        Range::AnonymousTag(params)
            => descriptor.range = range::RegistryTagRange {ident: None, tag: params.tag.clone()}.into(),

        _ => {},
    };

    if has_builtin_patch(&descriptor.ident) {
        descriptor.range = range::PatchRange {
            inner: Box::new(UrlEncoded::new(descriptor.clone())),
            path: "<builtin>".to_string(),
        }.into();

        descriptor.parent = Some(resolution.locator.clone());
    }
}

pub fn normalize_resolutions(context: &InstallContext<'_>, resolution: &Resolution) -> (BTreeMap<Ident, Descriptor>, BTreeMap<Ident, PeerRange>) {
    let project
        = context.project.expect("The project is required to normalize resolutions");

    let mut dependencies
        = resolution.dependencies.clone();

    let mut peer_dependencies
        = resolution.peer_dependencies.clone();

    for (descriptor, extension) in project.config.project.package_extensions.value.iter() {
        if descriptor.ident == resolution.locator.ident && descriptor.range.check(&resolution.version) {
            for (dependency, range) in extension.dependencies.iter() {
                dependencies.insert(dependency.clone(), range.clone());
            }

            for (peer_dependency, range) in extension.peer_dependencies.iter() {
                peer_dependencies.insert(peer_dependency.clone(), range.clone());
            }
        }
    }

    // Some protocols need to know about the package that declares the
    // dependency (for example the `portal:` protocol, which always points
    // to a location relative to the parent package. We mutate the
    // descriptors for these protocols to "bind" them to a particular
    // parent descriptor. In effect, it means we're creating a unique
    // version of the package, which will be resolved / fetched
    // independently from any other.
    //
    for descriptor in dependencies.values_mut() {
        normalize_resolution(context, descriptor, resolution, true);
    }

    for name in peer_dependencies.keys().filter(|ident| ident.scope() != Some("@types")).cloned().collect::<Vec<_>>() {
        peer_dependencies.entry(name.type_ident())
            .or_insert(range::SemverPeerRange {range: zpm_semver::Range::from_file_string("*").unwrap()}.into());
    }

    (dependencies, peer_dependencies)
}
