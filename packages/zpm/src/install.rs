use std::{collections::{BTreeMap, BTreeSet}, hash::Hash, marker::PhantomData, sync::LazyLock};

use chrono::{DateTime, Utc};
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use zpm_config::PackageExtension;
use zpm_primitives::{Descriptor, GitRange, Ident, Locator, PatchRange, PeerRange, Range, Reference, RegistrySemverRange, RegistryTagRange, SemverDescriptor, SemverPeerRange, WorkspaceIdentRange};
use zpm_utils::{Hash64, IoResultExt, Path, System, ToHumanString, UrlEncoded};
use bincode::{Decode, Encode};
use serde::{Deserialize, Serialize};
use zpm_utils::{FromFileString, ToFileString};

use crate::{
    build,
    cache::CompositeCache,
    constraints::check_constraints,
    content_flags::ContentFlags,
    error::Error,
    fetchers::{PackageData, SyncFetchAttempt, fetch_locator, patch::has_builtin_patch, try_fetch_locator_sync},
    graph::{GraphCache, GraphIn, GraphOut, GraphTasks},
    linker,
    lockfile::{Lockfile, LockfileEntry, LockfileMetadata},
    primitives_exts::RangeExt,
    project::{InstallMode, Project},
    report::{ReportContext, async_section, current_report, with_context_result},
    resolvers::{Resolution, SyncResolutionAttempt, resolve_descriptor, resolve_locator, try_resolve_descriptor_sync, validate_resolution}, tree_resolver::{ResolutionTree, TreeResolver},
};

#[derive(Clone)]
pub struct InstallContext<'a> {
    pub package_cache: Option<&'a CompositeCache>,
    pub project: Option<&'a Project>,
    pub systems: Option<&'a Vec<System>>,
    pub check_checksums: bool,
    pub check_resolutions: bool,
    pub prune_dev_dependencies: bool,
    pub enforced_resolutions: BTreeMap<Descriptor, Locator>,
    pub refresh_lockfile: bool,
    pub install_time: DateTime<Utc>,
    pub mode: Option<InstallMode>,
}

impl<'a> Default for InstallContext<'a> {
    fn default() -> Self {
        Self {
            package_cache: None,
            project: None,
            systems: None,
            check_checksums: false,
            check_resolutions: false,
            prune_dev_dependencies: false,
            enforced_resolutions: BTreeMap::new(),
            refresh_lockfile: false,
            install_time: Utc::now(),
            mode: None,
        }
    }
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

    pub fn set_enforced_resolutions(mut self, enforced_resolutions: BTreeMap<Descriptor, Locator>) -> Self {
        self.enforced_resolutions = enforced_resolutions;
        self
    }

    pub fn set_prune_dev_dependencies(mut self, prune_dev_dependencies: bool) -> Self {
        self.prune_dev_dependencies = prune_dev_dependencies;
        self
    }

    pub fn set_refresh_lockfile(mut self, refresh_lockfile: bool) -> Self {
        self.refresh_lockfile = refresh_lockfile;
        self
    }

    pub fn set_mode(mut self, mode: Option<InstallMode>) -> Self {
        self.mode = mode;
        self
    }

    pub fn with_systems(mut self, systems: Option<&'a Vec<System>>) -> Self {
        self.systems = systems;
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
            InstallOpResult::Resolved(resolution) => {
                resolution
            },

            _ => {
                panic!("Expected a resolved result; got {:?}", self)
            },
        }
    }

    pub fn into_fetched(self) -> FetchResult {
        match self {
            InstallOpResult::Fetched(fetch) => {
                fetch
            },

            _ => {
                panic!("Expected a fetched result; got {:?}", self)
            },
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
            InstallOpResult::Fetched(fetch) => {
                fetch
            },

            _ => {
                panic!("Expected a fetched result; got {:?}", self)
            },
        }
    }

    pub fn as_resolved_locator(&self) -> &Locator {
        match self {
            InstallOpResult::Resolved(params) => {
                &params.resolution.locator
            },

            InstallOpResult::Pinned(params) => {
                &params.locator
            },

            _ => {
                panic!("Expected a resolved locator; got {:?}", self)
            },
        }
    }
}

impl<'a> GraphOut<InstallContext<'a>, InstallOp<'a>> for InstallOpResult {
    fn graph_follow_ups(&self, op: &InstallOp<'a>, ctx: &InstallContext<'a>) -> Vec<InstallOp<'a>> {
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
                let systems
                    = ctx.systems.unwrap();

                let mut follow_ups = vec![InstallOp::Fetch {
                    locator: resolution.locator.clone(),
                    is_mock_request: !resolution.requirements.validate_any(systems),
                }];

                let transitive_dependencies = resolution.dependencies
                    .values()
                    .cloned()
                    .map(|dependency| InstallOp::Resolve {descriptor: dependency})
                    .chain(resolution.variants.iter().map(|variant| InstallOp::Refresh {locator: variant.locator.clone()}));

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
        is_mock_request: bool,
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
                    dependencies.push(InstallOp::Fetch {locator: parent.as_ref().clone(), is_mock_request: false});
                    resolved_it.next();
                }

                if let Some(inner_locator) = locator.reference.inner_locator().cloned() {
                    dependencies.push(InstallOp::Fetch {locator: inner_locator, is_mock_request: false});
                }
            },

            InstallOp::Resolve {descriptor} => {
                if let Some(parent) = &descriptor.parent {
                    dependencies.push(InstallOp::Fetch {locator: parent.clone(), is_mock_request: false});
                    resolved_it.next();
                }

                if let Some(mut inner_descriptor) = descriptor.range.inner_descriptor().cloned() {
                    if inner_descriptor.range.details().require_binding {
                        inner_descriptor.parent = descriptor.parent.clone();
                    }

                    dependencies.push(InstallOp::Resolve {descriptor: inner_descriptor});
                    let patch_resolution = resolved_it.next();

                    if let Some(result) = patch_resolution {
                        let patch_resolved_locator = result.as_resolved_locator();
                        dependencies.push(InstallOp::Fetch {locator: patch_resolved_locator.clone(), is_mock_request: false});
                    }
                }
            },

            InstallOp::Fetch {locator, ..} => {
                if let Some(parent) = &locator.parent {
                    dependencies.push(InstallOp::Fetch {locator: parent.as_ref().clone(), is_mock_request: false});
                }

                if let Some(inner_locator) = locator.reference.inner_locator().cloned() {
                    dependencies.push(InstallOp::Fetch {locator: inner_locator, is_mock_request: false});
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
                current_report().await.as_ref().map(|report| {
                    report.counters.resolution_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                });

                with_context_result(ReportContext::Descriptor(descriptor.clone()), async {
                    tokio::time::timeout(
                        timeout,
                        validate_resolution(context.clone(), descriptor.clone(), locator.clone(), dependencies)
                    ).await.map_err(|_| Error::TaskTimeout)??;

                    Ok(InstallOpResult::Validated)
                }).await
            },

            InstallOp::Refresh {locator} => {
                current_report().await.as_ref().map(|report| {
                    report.counters.resolution_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                });

                with_context_result(ReportContext::Locator(locator.clone()), async {
                    let future = tokio::time::timeout(
                        timeout,
                        resolve_locator(context.clone(), locator.clone(), dependencies)
                    ).await.map_err(|_| Error::TaskTimeout)?;

                    Ok(InstallOpResult::Resolved(future?))
                }).await
            },

            InstallOp::Resolve {descriptor} => {
                if !descriptor.range.details().transient_resolution {
                    current_report().await.as_ref().map(|report| {
                        report.counters.resolution_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    });
                }

                with_context_result(ReportContext::Descriptor(descriptor.clone()), async {
                    let dependencies = match try_resolve_descriptor_sync(context.clone(), descriptor.clone(), dependencies)? {
                        SyncResolutionAttempt::Success(result) => return Ok(InstallOpResult::Resolved(result)),
                        SyncResolutionAttempt::Failure(dependencies) => dependencies,
                    };

                    let future = tokio::time::timeout(
                        timeout,
                        resolve_descriptor(context.clone(), descriptor.clone(), dependencies)
                    ).await.map_err(|_| Error::TaskTimeout)?;

                    Ok(InstallOpResult::Resolved(future?))
                }).await
            },

            InstallOp::Fetch {locator, is_mock_request} => {
                with_context_result(ReportContext::Locator(locator.clone()), async {
                    let dependencies = match try_fetch_locator_sync(context.clone(), &locator, is_mock_request, dependencies)? {
                        SyncFetchAttempt::Success(result) => return Ok(InstallOpResult::Fetched(result)),
                        SyncFetchAttempt::Failure(dependencies) => dependencies,
                    };

                    let future = tokio::time::timeout(
                        timeout,
                        fetch_locator(context.clone(), &locator.clone(), is_mock_request, dependencies)
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
            let range_details
                = descriptor.range.details();

            if range_details.transient_resolution {
                return None;
            }

            let enforced_resolution
                = ctx.enforced_resolutions.get(descriptor);

            if let Some(locator) = self.lockfile.resolutions.get(descriptor) {
                if enforced_resolution.map_or(true, |enforced_resolution| locator == enforced_resolution) {
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

            if let Some(locator) = enforced_resolution {
                return Some(InstallOpResult::Pinned(PinnedResult {
                    locator: locator.clone(),
                }));
            }
        }

        None
    }
}

#[derive(Clone, Debug, Encode, Decode, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct InstallState {
    pub last_installed_at: u128,
    pub content_flags: BTreeMap<Locator, ContentFlags>,
    pub resolution_tree: ResolutionTree,
    pub descriptor_to_locator: BTreeMap<Descriptor, Locator>,
    pub normalized_resolutions: BTreeMap<Locator, Resolution>,
    pub packages_by_location: BTreeMap<Path, Locator>,
    pub locations_by_package: BTreeMap<Locator, Path>,
    pub optional_packages: BTreeSet<Locator>,
    pub disabled_locators: BTreeSet<Locator>,
    pub conditional_locators: BTreeSet<Locator>,
}

#[derive(Clone, Default)]
pub struct Install {
    pub lockfile: Lockfile,
    pub lockfile_changed: bool,
    pub package_data: BTreeMap<Locator, PackageData>,
    pub install_state: InstallState,
    pub roots: BTreeSet<Descriptor>,
    pub skip_build: bool,
    pub skip_lockfile_update: bool,
    pub constraints_check: bool,
}

#[derive(Debug)]
pub struct InstallResult {
    pub package_data: BTreeMap<Locator, PackageData>,
}

impl Install {
    pub async fn link_and_build(mut self, project: &mut Project) -> Result<InstallResult, Error> {
        self.install_state.last_installed_at = project.last_changed_at;

        let link_future
            = linker::link_project(project, &mut self);

        let link_result
            = async_section("Linking the project", link_future).await?;

        for (location, locator) in &link_result.packages_by_location {
            self.install_state.locations_by_package.insert(locator.clone(), location.clone());
        }

        self.install_state.packages_by_location
            = link_result.packages_by_location;

        project.attach_install_state(self.install_state)?;

        if !self.skip_lockfile_update {
            project.write_lockfile(&self.lockfile)?;
        }

        if !self.skip_build && !link_result.build_requests.entries.is_empty() {
            let build_future
                = build::BuildManager::new(link_result.build_requests).run(project);

            let build_result
                = async_section("Building the project", build_future).await?;

            if !build_result.build_errors.is_empty() {
                return Err(Error::SilentError);
            }
        }

        if self.constraints_check {
            async_section("Checking constraints", async {
                let output
                    = check_constraints(project, false).await?;

                if !output.is_empty() {
                    return Err(Error::AutoConstraintsError);
                }

                Ok(())
            }).await?;
        }

        project.ignore_path()
            .with_join_str(".gitignore")
            .fs_change("*", false)
            .ok_missing()?;

        Ok(InstallResult {
            package_data: self.package_data,
        })
    }
}

pub struct InstallManager<'a> {
    initial_lockfile: Lockfile,
    context: InstallContext<'a>,
    previous_state: Option<&'a InstallState>,
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
            initial_lockfile: Lockfile::new(),
            context: InstallContext::default(),
            previous_state: None,
            result: Install::default(),
        }
    }

    pub fn with_context(mut self, context: InstallContext<'a>) -> Self {
        self.context = context;
        self
    }

    pub fn with_previous_state(mut self, previous_state: Option<&'a InstallState>) -> Self {
        self.previous_state = previous_state;
        self
    }

    pub fn with_lockfile(mut self, lockfile: Lockfile) -> Self {
        self.initial_lockfile = lockfile;
        self
    }

    pub fn with_roots(mut self, roots: BTreeSet<Descriptor>) -> Self {
        self.result.roots = roots;
        self
    }

    pub fn with_constraints_check(mut self, constraints_check: bool) -> Self {
        self.result.constraints_check = constraints_check;
        self
    }

    pub fn with_skip_lockfile_update(mut self, skip_lockfile_update: bool) -> Self {
        self.result.skip_lockfile_update = skip_lockfile_update;
        self
    }

    pub async fn resolve_and_fetch(mut self) -> Result<Install, Error> {
        let cache
            = InstallCache::new(self.initial_lockfile.clone());

        let mut graph
            = GraphTasks::new(self.context.clone(), cache);

        for descriptor in self.result.roots.clone() {
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
                    self.record_fetch(locator, package_data)?;
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
                    = Hash64::from_data(&archive_data);

                Ok((locator, checksum))
            })
            .collect::<Result<BTreeMap<_, _>, Error>>()?;

        for entry in self.result.lockfile.entries.values_mut() {
            let package_data = self.result.package_data
                .get(&entry.resolution.locator)
                .unwrap_or_else(|| panic!("Expected a matching package data to be found for any fetched locator; not found for {}.", entry.resolution.locator.to_file_string()));

            let previous_entry
                = self.initial_lockfile.entries.get(&entry.resolution.locator);

            let previous_checksum = previous_entry
                .and_then(|s| s.checksum.as_ref());

            let mut checksum = package_data.checksum()
                .or_else(|| previous_checksum.cloned())
                .or_else(|| late_checksums.get(&entry.resolution.locator).cloned());

            let is_conditional_locator
                = self.result.install_state.conditional_locators
                    .contains(&entry.resolution.locator);

            if is_conditional_locator {
                checksum = None;
            }

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

            entry.checksum = checksum;
        }

        self.result.install_state.resolution_tree = TreeResolver::default()
            .with_resolutions(&self.result.install_state.descriptor_to_locator, &self.result.install_state.normalized_resolutions)?
            .with_roots(self.result.roots.clone())
            .run();

        self.result.lockfile.resolutions = self.result.install_state.descriptor_to_locator.clone();
        self.result.lockfile_changed = self.result.lockfile != self.initial_lockfile;

        self.result.skip_build = self.context.mode == Some(InstallMode::SkipBuild);

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
        });

        if resolution.requirements.is_conditional() {
            let systems
                = self.context.systems.unwrap();

            self.result.install_state.conditional_locators.insert(resolution.locator.clone());

            if !resolution.requirements.validate_any(systems) {
                self.result.install_state.disabled_locators.insert(resolution.locator.clone());
            }
        }

        if let Some(package_data) = package_data {
            self.record_fetch(resolution.locator, package_data)?;
        }

        Ok(())
    }

    fn record_descriptor(&mut self, descriptor: Descriptor, locator: Locator) {
        self.result.install_state.descriptor_to_locator.insert(descriptor, locator);
    }

    fn record_fetch(&mut self, locator: Locator, package_data: PackageData) -> Result<(), Error> {
        let content_flags
            = self.previous_state
                .and_then(|previous_state| previous_state.content_flags.get(&locator))
                .cloned()
                .map_or_else(|| ContentFlags::extract(&locator, &package_data), Ok)?;

        self.result.package_data.insert(locator.clone(), package_data);

        self.result.install_state.content_flags.insert(locator, content_flags);

        Ok(())
    }
}

fn normalize_resolution(context: &InstallContext<'_>, descriptor: &mut Descriptor, resolution: &Resolution, apply_overrides: bool) -> () {
    if apply_overrides {
        let candidate_resolutions = context.project
            .expect("The project is required to normalize resolutions, as it may be impacted by the project's overrides")
            .root_workspace()
            .manifest
            .resolutions
            .get_by_ident(&descriptor.ident);

        let resolution_override = candidate_resolutions
            .and_then(|overrides| {
                overrides.iter().find_map(|(rule, range)| {
                    rule.apply(&resolution.locator, &resolution.version, descriptor, range)
                })
            });

        if let Some(replacement_range) = resolution_override {
            descriptor.range = replacement_range;

            if descriptor.range.details().require_binding {
                let root_workspace = context.project
                    .expect("The project is required to bind a parent to a descriptor")
                    .root_workspace();

                descriptor.parent = Some(root_workspace.locator());
            } else {
                descriptor.parent = None;
            }
        } else if descriptor.range.details().require_binding {
            descriptor.parent = Some(resolution.locator.clone());
        }

        if has_builtin_patch(&descriptor.ident) {
            descriptor.range = PatchRange {
                inner: Box::new(UrlEncoded::new(descriptor.clone())),
                path: "<builtin>".to_string(),
            }.into();
        }
    }

    match &mut descriptor.range {
        Range::Patch(params) => {
            normalize_resolution(context, &mut params.inner.as_mut().0, resolution, false);
        },

        Range::AnonymousSemver(params) => {
            descriptor.range = RegistrySemverRange {
                ident: None,
                range: params.range.clone(),
            }.into();
        },

        Range::AnonymousTag(params) => {
            descriptor.range = RegistryTagRange {
                ident: None,
                tag: params.tag.clone(),
            }.into();
        },

        _ => {},
    };
}

const BUILTIN_EXTENSIONS_JSON: &str = include_str!("../data/builtin-extensions.json");

static BUILTIN_EXTENSIONS: LazyLock<BTreeMap<SemverDescriptor, PackageExtension>> = LazyLock::new(|| {
    let extensions: Vec<(SemverDescriptor, PackageExtension)>
        = serde_json::from_str(BUILTIN_EXTENSIONS_JSON)
            .expect("Failed to parse builtin extensions JSON");

    let extension_map = extensions
        .into_iter()
        .collect::<BTreeMap<_, _>>();

    extension_map
});

pub fn normalize_resolutions(context: &InstallContext<'_>, resolution: &Resolution) -> (BTreeMap<Ident, Descriptor>, BTreeMap<Ident, PeerRange>) {
    let project
        = context.project.expect("The project is required to normalize resolutions");

    let mut dependencies
        = resolution.dependencies.clone();

    let mut peer_dependencies
        = resolution.peer_dependencies.clone();

    if let Reference::Git(params) = &resolution.locator.reference {
        for descriptor in resolution.dependencies.values() {
            let updated_range = match &descriptor.range {
                Range::WorkspaceIdent(WorkspaceIdentRange {ident, ..}) => {
                    let mut workspace_git_range
                        = params.git.to_git_range();

                    workspace_git_range.prepare_params.workspace = Some(ident.to_file_string());

                    Some(Range::Git(GitRange {
                        git: workspace_git_range,
                    }))
                }

                Range::WorkspaceMagic(_) |
                Range::WorkspaceSemver(_) => {
                    let mut workspace_git_range
                        = params.git.to_git_range();

                    workspace_git_range.prepare_params.workspace = Some(descriptor.ident.to_file_string());

                    Some(Range::Git(GitRange {
                        git: workspace_git_range,
                    }))
                },

                _ => {
                    None
                },
            };

            if let Some(updated_range) = updated_range {
                dependencies.insert(
                    descriptor.ident.clone(),
                    Descriptor::new(descriptor.ident.clone(), updated_range),
                );
            }
        }
    }

    for (descriptor, extension) in project.config.settings.package_extensions.iter() {
        if descriptor.ident == resolution.locator.ident && descriptor.range.check(&resolution.version) {
            for (dependency, range) in extension.dependencies.iter() {
                if !dependencies.contains_key(dependency) {
                    dependencies.insert(dependency.clone(), Descriptor::new_bound(dependency.clone(), range.value.clone(), None));
                }
            }

            for (peer_dependency, range) in extension.peer_dependencies.iter() {
                if !peer_dependencies.contains_key(peer_dependency) {
                    peer_dependencies.insert(peer_dependency.clone(), range.value.clone());
                }
            }
        }
    }

    for (descriptor, extension) in BUILTIN_EXTENSIONS.iter() {
        if descriptor.ident == resolution.locator.ident && descriptor.range.check(&resolution.version) {
            for (dependency, range) in extension.dependencies.iter() {
                if !dependencies.contains_key(dependency) {
                    dependencies.insert(dependency.clone(), Descriptor::new_bound(dependency.clone(), range.value.clone(), None));
                }
            }

            for (peer_dependency, range) in extension.peer_dependencies.iter() {
                if !peer_dependencies.contains_key(peer_dependency) {
                    peer_dependencies.insert(peer_dependency.clone(), range.value.clone());
                }
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
            .or_insert(SemverPeerRange {range: zpm_semver::Range::from_file_string("*").unwrap()}.into());
    }

    (dependencies, peer_dependencies)
}
