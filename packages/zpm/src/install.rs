use std::{collections::{BTreeMap, BTreeSet, HashMap, HashSet}, sync::{Arc, LazyLock}};

use chrono::{DateTime, Utc};
use futures::future::{BoxFuture, FutureExt};
use futures::stream::{FuturesUnordered, StreamExt};
use itertools::Itertools;
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use zpm_config::PackageExtension;
use zpm_primitives::{Descriptor, GitRange, Ident, Locator, PatchRange, PeerRange, Range, Reference, RegistrySemverRange, RegistryTagRange, SemverDescriptor, SemverPeerRange, WorkspaceIdentRange};
use zpm_utils::{Hash64, Hash64Writer, IoResultExt, Path, System, ToHumanString, UrlEncoded};
use rkyv::Archive;
use serde::{Deserialize, Serialize};
use zpm_utils::{FromFileString, ToFileString};

use crate::{
    build,
    cache::CompositeCache,
    constraints::check_constraints,
    content_flags::ContentFlags,
    error::Error,
    fetchers::{PackageData, SyncFetchAttempt, fetch_locator, patch::has_builtin_patch, try_fetch_locator_sync},
    graph::WaitMap,
    linker,
    lockfile::{Lockfile, LockfileEntry, LockfileMetadata},
    primitives_exts::{InnerDependencyKind, RangeExt},
    project::{InstallMode, Project},
    report::{ReportContext, async_section, current_report, with_context_result},
    resolvers::{Resolution, SyncResolutionAttempt, catalog::lookup_catalog_entry, resolve_descriptor, resolve_locator, try_resolve_descriptor_sync}, tree_resolver::{ResolutionTree, TreeResolver},
};

#[derive(Clone)]
pub struct InstallContext<'a> {
    pub package_cache: Option<&'a CompositeCache>,
    pub project: Option<&'a Project>,
    pub systems: Option<&'a Vec<System>>,
    pub check_checksums: bool,
    pub check_resolutions: bool,
    pub prune_dev_dependencies: bool,
    pub enforced_resolutions: BTreeMap<Descriptor, Option<Locator>>,
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

    pub fn set_enforced_resolutions(mut self, enforced_resolutions: BTreeMap<Descriptor, Option<Locator>>) -> Self {
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
pub struct ResolutionResult {
    pub resolution: Resolution,
    pub original_resolution: Resolution,
    pub package_data: Option<PackageData>,
}

pub trait IntoResolutionResult {
    fn into_resolution_result(self, context: &InstallContext<'_>) -> Result<ResolutionResult, Error>;
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

    pub fn new_mock(archive_path: Path, package_directory: Path) -> Self {
        Self::new(PackageData::MissingZip {
            archive_path,
            context_directory: package_directory.clone(),
            package_directory,
        })
    }
}

impl IntoResolutionResult for FetchResult {
    fn into_resolution_result(self, context: &InstallContext<'_>) -> Result<ResolutionResult, Error> {
        let mut resolution = self.resolution
            .expect("Expected this fetch result to contain a resolution record to be convertible into a resolution result");

        let original_resolution = resolution.clone();

        let (dependencies, peer_dependencies)
            = normalize_resolutions(context, &resolution)?;

        resolution.dependencies = dependencies;
        resolution.peer_dependencies = peer_dependencies;

        Ok(ResolutionResult {
            resolution,
            original_resolution,
            package_data: Some(self.package_data),
        })
    }
}

/// Shared context for the WaitMap-based resolution/fetch pipeline.
struct InstallMaps {
    resolution_map: Arc<WaitMap<Descriptor, ResolutionResult>>,
    fetch_map: Arc<WaitMap<Locator, FetchResult>>,
}

/// Resolve a single descriptor: runs `get_or_init` for the resolution + starts
/// the fetch, then returns child descriptors to be resolved next.
///
/// The resolution logic is split: `get_or_init` runs only the core resolution +
/// starts the fetch. Children are returned (not recursed into) so the caller
/// can drive them iteratively, avoiding stack overflow on deep dependency trees.
async fn resolve_one<'a>(
    descriptor: Descriptor,
    ctx: &'a InstallContext<'a>,
    lockfile: &'a Lockfile,
    maps: &'a InstallMaps,
) -> Vec<Descriptor> {
    let cell
        = maps.resolution_map.entry(descriptor.clone());

    let result = cell.get_or_init(|| {
        resolve_descriptor_impl(descriptor.clone(), ctx, lockfile, maps)
    }).await;

    let Ok(result) = result else {
        return vec![];
    };

    let children
        = result.resolution.dependencies
            .values()
            .chain(result.resolution.variants.iter())
            .cloned()
            .collect();

    children
}

/// Iteratively resolve all descriptors starting from the given roots.
/// Uses `FuturesUnordered` to process descriptors concurrently without
/// recursive async calls (which would overflow the stack on deep trees).
async fn resolve_all<'a>(
    roots: impl IntoIterator<Item = Descriptor>,
    ctx: &'a InstallContext<'a>,
    lockfile: &'a Lockfile,
    maps: &'a InstallMaps,
) {
    let mut in_flight
        = FuturesUnordered::new();

    for descriptor in roots {
        in_flight.push(resolve_one(descriptor, ctx, lockfile, maps));
    }

    while let Some(children) = in_flight.next().await {
        for child in children {
            // Only schedule children whose resolution hasn't been started yet.
            // This prevents infinite loops on cyclic dependency graphs.
            let cell
                = maps.resolution_map.entry(child.clone());

            if !cell.initialized() {
                in_flight.push(resolve_one(child, ctx, lockfile, maps));
            }
        }
    }
}

/// The actual resolution logic for a single descriptor.
/// Returns the resolution result; does NOT recurse into children (that happens
/// in `resolve_one` after the OnceCell init completes, via the `resolve_all` worklist).
///
/// Returns a `BoxFuture` to support recursive calls for inner descriptors
/// (e.g. alias->inner, patch->inner) without causing infinite future sizes.
fn resolve_descriptor_impl<'a>(
    descriptor: Descriptor,
    ctx: &'a InstallContext<'a>,
    lockfile: &'a Lockfile,
    maps: &'a InstallMaps,
) -> BoxFuture<'a, Result<ResolutionResult, Arc<Error>>> {
    async move {
        let timeout
            = std::time::Duration::from_secs(600);

        // Phase 1: Check lockfile cache
        let cached
            = check_resolution_cache(ctx, lockfile, &descriptor)
                .map_err(Arc::new)?;

        if let Some(cached) = cached {
            match cached {
                CacheHit::Full(result) => {
                    start_fetch(&result, ctx, maps).await;
                    return Ok(result);
                },

                CacheHit::Pinned(locator) => {
                    // Inline Refresh: wait for locator prerequisites, then resolve_locator
                    let refresh_deps
                        = build_locator_fetch_deps(&locator, maps, ctx).await?;

                    current_report().await.as_ref().map(|report| {
                        report.counters.resolution_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    });

                    let result = with_context_result(ReportContext::Locator(locator.clone()), async {
                        tokio::time::timeout(
                            timeout,
                            resolve_locator(ctx.clone(), locator.clone(), refresh_deps)
                        ).await.map_err(|_| Error::TaskTimeout)?
                    }).await.map_err(Arc::new)?;

                    start_fetch(&result, ctx, maps).await;
                    return Ok(result);
                },
            }
        }

        // Phase 2: Await prerequisites and build the dependencies vector
        let mut dependencies
            = vec![];

        // Parent fetch
        if let Some(parent) = &descriptor.parent {
            let parent_fetch
                = await_fetch(parent, maps, ctx).await?;

            dependencies.push(InstallOpResult::Fetched(parent_fetch));
        }

        // Inner descriptor resolution + maybe inner fetch
        if let Some(mut inner_descriptor) = descriptor.range.inner_descriptor() {
            if inner_descriptor.range.details().require_binding {
                inner_descriptor.parent = descriptor.parent.clone();
            }

            // Resolve the inner descriptor (this is NOT a regular child dep, it's
            // a structural prerequisite like alias->inner or patch->inner).
            // Uses get_or_init directly; inner descriptor's children will be
            // discovered by the resolve_all worklist via resolve_one.
            let inner_cell
                = maps.resolution_map.entry(inner_descriptor.clone());

            inner_cell.get_or_init(|| {
                resolve_descriptor_impl(inner_descriptor.clone(), ctx, lockfile, maps)
            }).await;

            let inner_result
                = get_resolution_result(&inner_descriptor, maps).await?;
            let inner_locator
                = inner_result.resolution.locator.clone();

            let inner_dep_kind
                = descriptor.range.inner_dependency();

            match inner_dep_kind {
                Some(InnerDependencyKind::Resolution) => {
                    dependencies.push(InstallOpResult::Resolved(inner_result));
                },

                Some(InnerDependencyKind::Fetch) => {
                    let mut fetch_result
                        = await_fetch(&inner_locator, maps, ctx).await?;

                    // The shared fetch_map may contain a mock fetch (MissingZip)
                    // if the package doesn't match the current system requirements.
                    // The outer package (e.g. a patch) needs the real contents to
                    // apply the patch, so we re-fetch directly with is_mock_request
                    // =false, bypassing the shared map. The disk-level package cache
                    // still deduplicates the actual download.
                    if matches!(fetch_result.package_data, PackageData::MissingZip {..}) {
                        fetch_result = fetch_locator_impl(inner_locator, false, ctx, maps).await?;
                    }

                    dependencies.push(InstallOpResult::Resolved(inner_result));
                    dependencies.push(InstallOpResult::Fetched(fetch_result));
                },

                None => {},
            }
        }

        // Phase 3: Resolve
        if !descriptor.range.details().transient_resolution {
            current_report().await.as_ref().map(|report| {
                report.counters.resolution_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            });
        }

        let result = with_context_result(ReportContext::Descriptor(descriptor.clone()), async {
            let dependencies = match try_resolve_descriptor_sync(ctx.clone(), descriptor.clone(), dependencies) {
                Ok(SyncResolutionAttempt::Success(result)) => return Ok(result),
                Ok(SyncResolutionAttempt::Failure(dependencies)) => dependencies,
                Err(e) => return Err(e),
            };

            tokio::time::timeout(
                timeout,
                resolve_descriptor(ctx.clone(), descriptor.clone(), dependencies)
            ).await.map_err(|_| Error::TaskTimeout)?
        }).await.map_err(Arc::new)?;

        // Phase 4: Start fetch (children are handled by resolve_all worklist after init completes)
        start_fetch(&result, ctx, maps).await;

        Ok(result)
    }.boxed()
}

/// Start a fetch for the resolved locator.
async fn start_fetch<'a>(
    result: &ResolutionResult,
    ctx: &'a InstallContext<'a>,
    maps: &'a InstallMaps,
) {
    let systems
        = ctx.systems.unwrap();
    let is_mock_request
        = !result.resolution.requirements.validate_any(systems);
    let locator
        = result.resolution.locator.clone();

    ensure_fetched(locator, is_mock_request, ctx, maps).await;
}

/// Ensure a locator is fetched. Uses the fetch WaitMap for deduplication.
fn ensure_fetched<'a>(
    locator: Locator,
    is_mock_request: bool,
    ctx: &'a InstallContext<'a>,
    maps: &'a InstallMaps,
) -> BoxFuture<'a, ()> {
    async move {
        let cell
            = maps.fetch_map.entry(locator.clone());

        cell.get_or_init(|| async {
            fetch_locator_impl(locator, is_mock_request, ctx, maps).await
        }).await;
    }.boxed()
}

/// The actual fetch logic for a single locator.
async fn fetch_locator_impl<'a>(
    locator: Locator,
    is_mock_request: bool,
    ctx: &'a InstallContext<'a>,
    maps: &'a InstallMaps,
) -> Result<FetchResult, Arc<Error>> {
    let timeout
        = std::time::Duration::from_secs(600);

    // Build fetch dependencies (same order as old graph_dependencies for InstallOp::Fetch)
    let mut dependencies: Vec<InstallOpResult>
        = vec![];

    if let Some(parent) = &locator.parent {
        let parent_fetch
            = await_fetch(parent.as_ref(), maps, ctx).await?;

        dependencies.push(InstallOpResult::Fetched(parent_fetch));
    }

    if let Some(inner_locator) = locator.reference.inner_locator().cloned() {
        let inner_fetch
            = await_fetch(&inner_locator, maps, ctx).await?;

        dependencies.push(InstallOpResult::Fetched(inner_fetch));
    }

    with_context_result(ReportContext::Locator(locator.clone()), async {
        let dependencies = match try_fetch_locator_sync(ctx.clone(), &locator, is_mock_request, dependencies) {
            Ok(SyncFetchAttempt::Success(result)) => return Ok(result),
            Ok(SyncFetchAttempt::Failure(dependencies)) => dependencies,
            Err(e) => return Err(e),
        };

        let future = tokio::time::timeout(
            timeout,
            fetch_locator(ctx.clone(), &locator, is_mock_request, dependencies)
        ).await.map_err(|_| Error::TaskTimeout)?;

        if is_mock_request {
            if let Ok(result) = future.as_ref() {
                if let FetchResult {package_data: PackageData::Zip {..}, ..} = result {
                    current_report().await.as_ref().map(|report| {
                        report.warn(format!("Mock request for {} returned a zip package; this should not happen.", locator.to_print_string()));
                    });
                }
            }
        }

        future
    }).await.map_err(Arc::new)
}

/// Ensure a locator is fetched and return its result by awaiting the fetch map.
/// If the fetch hasn't been initiated yet, this will initiate it (non-mock).
/// Uses BoxFuture because fetch_locator_impl can recursively call await_fetch.
fn await_fetch<'a>(locator: &Locator, maps: &'a InstallMaps, ctx: &'a InstallContext<'a>) -> BoxFuture<'a, Result<FetchResult, Arc<Error>>> {
    let cell
        = maps.fetch_map
            .entry(locator.clone());

    let locator_clone
        = locator.clone();

    async move {
        let result = cell.get_or_init(|| async {
            fetch_locator_impl(locator_clone, false, ctx, maps).await
        }).await;

        match result {
            Ok(v) => Ok(v.clone()),
            Err(e) => Err(e.clone()),
        }
    }.boxed()
}

/// Get a resolution result from the resolution map. The entry must already be initialized.
async fn get_resolution_result(descriptor: &Descriptor, maps: &InstallMaps) -> Result<ResolutionResult, Arc<Error>> {
    let cell
        = maps.resolution_map
            .entry(descriptor.clone());

    let result = cell.get_or_init(|| async {
        Err(Arc::new(Error::MissingResolution(descriptor.clone())))
    }).await;

    match result {
        Ok(v) => Ok(v.clone()),
        Err(e) => Err(e.clone()),
    }
}

/// Build the fetch dependencies vector for a locator (for resolve_locator / Refresh).
async fn build_locator_fetch_deps<'a>(locator: &Locator, maps: &'a InstallMaps, ctx: &'a InstallContext<'a>) -> Result<Vec<InstallOpResult>, Arc<Error>> {
    let mut dependencies
        = vec![];

    if let Some(parent) = &locator.parent {
        let parent_fetch
            = await_fetch(parent.as_ref(), maps, ctx).await?;

        dependencies.push(InstallOpResult::Fetched(parent_fetch));
    }

    if let Some(inner_locator) = locator.reference.inner_locator().cloned() {
        let inner_fetch
            = await_fetch(&inner_locator, maps, ctx).await?;

        dependencies.push(InstallOpResult::Fetched(inner_fetch));
    }

    Ok(dependencies)
}

enum CacheHit {
    Full(ResolutionResult),
    Pinned(Locator),
}

/// Check if a descriptor can be resolved from the lockfile cache.
fn check_resolution_cache(ctx: &InstallContext<'_>, lockfile: &Lockfile, descriptor: &Descriptor) -> Result<Option<CacheHit>, Error> {
    let range_details
        = descriptor.range.details();

    if range_details.transient_resolution {
        return Ok(None);
    }

    // enforced_resolutions semantics:
    // - None (not in map): use lockfile resolution if available
    // - Some(None): skip lockfile, force re-resolution
    // - Some(Some(locator)): force resolution to specific locator
    let enforced_resolution
        = ctx.enforced_resolutions.get(descriptor);

    // If Some(None), skip lockfile lookup entirely and force re-resolution
    if enforced_resolution == Some(&None) {
        return Ok(None);
    }

    // Get the enforced locator if any
    let enforced_locator
        = enforced_resolution.and_then(|opt| opt.as_ref());

    if let Some(locator) = lockfile.resolutions.get(descriptor) {
        if enforced_locator.map_or(true, |enforced| locator == enforced) {
            if lockfile.metadata.version != LockfileMetadata::new().version || ctx.refresh_lockfile {
                return Ok(Some(CacheHit::Pinned(locator.clone())));
            }

            let entry
                = lockfile.entries.get(locator)
                    .unwrap_or_else(|| panic!("Expected a matching resolution to be found in the lockfile for any resolved locator; not found for {}.", locator.to_print_string()));

            return Ok(Some(CacheHit::Full(entry.resolution.clone().into_resolution_result(ctx)?)));
        }
    }

    if let Some(locator) = enforced_locator {
        return Ok(Some(CacheHit::Pinned(locator.clone())));
    }

    Ok(None)
}

// Legacy types kept for compatibility with resolver/fetcher function signatures.
// These will be removed once the resolver/fetcher modules are refactored.
#[derive(Clone, Debug)]
pub enum InstallOpResult {
    Resolved(ResolutionResult),
    Fetched(FetchResult),
}

impl InstallOpResult {
    pub fn as_fetched(&self) -> &FetchResult {
        match self {
            InstallOpResult::Fetched(fetch) => fetch,
            _ => panic!("Expected a fetched result; got {:?}", self),
        }
    }

    pub fn as_resolved_locator(&self) -> &Locator {
        match self {
            InstallOpResult::Resolved(params) => &params.resolution.locator,
            _ => panic!("Expected a resolved locator; got {:?}", self),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq, Archive, rkyv::Serialize, rkyv::Deserialize)]
#[rkyv(serialize_bounds(__S: rkyv::ser::Writer + rkyv::ser::Allocator + rkyv::ser::Sharing, <__S as rkyv::rancor::Fallible>::Error: rkyv::rancor::Source))]
#[rkyv(deserialize_bounds(__D: rkyv::de::Pooling, <__D as rkyv::rancor::Fallible>::Error: rkyv::rancor::Source))]
#[rkyv(bytecheck(bounds(__C: rkyv::validation::ArchiveContext + rkyv::validation::SharedContext, <__C as rkyv::rancor::Fallible>::Error: rkyv::rancor::Source)))]
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
    pub island_descriptor_to_locator: BTreeMap<String, BTreeMap<Descriptor, Locator>>,
    pub island_normalized_resolutions: BTreeMap<String, BTreeMap<Locator, Resolution>>,
}

#[derive(Clone, Default)]
pub struct Install {
    pub lockfile: Lockfile,
    pub lockfile_changed: bool,
    pub package_data: BTreeMap<Locator, PackageData>,
    pub install_state: InstallState,
    pub roots: BTreeSet<Descriptor>,
    pub resolved_islands: Vec<crate::island::ResolvedIsland>,
    pub skip_build: bool,
    pub skip_link_step: bool,
    pub skip_lockfile_update: bool,
    pub constraints_check: bool,
}

#[derive(Debug)]
pub struct InstallResult {
    pub package_data: BTreeMap<Locator, PackageData>,
}

impl Install {
    pub async fn link_and_build(mut self, project: &mut Project) -> Result<InstallResult, Error> {
        if self.skip_link_step {
            project.attach_install_state(self.install_state)?;

            if !self.skip_lockfile_update {
                project.write_lockfile(&self.lockfile)?;
            }
        } else {
            self.install_state.last_installed_at = project.last_modified_at.as_nanos();

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

    pub fn with_skip_link_step(mut self, skip_link_step: bool) -> Self {
        self.result.skip_link_step = skip_link_step;
        self
    }

    pub fn with_skip_lockfile_update(mut self, skip_lockfile_update: bool) -> Self {
        self.result.skip_lockfile_update = skip_lockfile_update;
        self
    }

    pub async fn resolve_and_fetch(mut self) -> Result<Install, Error> {
        let maps = InstallMaps {
            resolution_map: Arc::new(WaitMap::new()),
            fetch_map: Arc::new(WaitMap::new()),
        };

        let lockfile
            = self.initial_lockfile.clone();

        // --- Island resolution ---
        // Resolve islands from project config and partition roots between
        // island resolution (pubgrub) and greedy resolution.
        let project = self.context.project;

        let resolved_islands = if let Some(project) = project {
            let island_config = &project.config.settings.unstable_islands;
            if !island_config.is_empty() {
                Some(crate::island::resolve_islands(island_config, &project.workspaces)?)
            } else {
                None
            }
        } else {
            None
        };

        // Island workspaces are fully handled by pubgrub — they don't go
        // through greedy resolution or the PnP linker. Remove them from roots.
        let island_workspace_idents: BTreeSet<Ident> = resolved_islands.as_ref()
            .map(|islands| islands.iter().flat_map(|i| i.workspace_idents.iter().cloned()).collect())
            .unwrap_or_default();

        self.result.roots.retain(|d| !island_workspace_idents.contains(&d.ident));
        let roots = self.result.roots.clone();

        async_section("Installing packages", async {
            let greedy_future = resolve_all(roots, &self.context, &lockfile, &maps);

            if let Some(ref islands) = resolved_islands {
                // Store resolved islands for use during linking
                self.result.resolved_islands = islands.clone();

                let island_futures = islands.iter().map(|island| {
                    crate::island::resolve_island(island, &self.context, &lockfile)
                });

                let (_, island_results) = futures::future::try_join(
                    async { greedy_future.await; Ok::<(), Error>(()) },
                    futures::future::try_join_all(island_futures),
                ).await?;

                // Merge island results into install state and fetch packages
                let mut island_locators = Vec::new();

                for island_result in island_results {
                    let island_id = island_result.island_id.clone();

                    // Store per-island mappings (kept separate from global maps
                    // to preserve island isolation)
                    self.result.install_state.island_descriptor_to_locator
                        .insert(island_id.clone(), island_result.descriptor_to_locator.clone());
                    self.result.install_state.island_normalized_resolutions
                        .insert(island_id.clone(), island_result.normalized_resolutions.clone());

                    // Lockfile entries are shared (for checksum tracking etc.)
                    for (locator, resolution) in &island_result.normalized_resolutions {
                        island_locators.push(locator.clone());
                        self.result.lockfile.entries
                            .entry(locator.clone())
                            .or_insert_with(|| LockfileEntry {
                                checksum: None,
                                resolution: resolution.clone(),
                            });
                    }

                    // Store island descriptor→locator in lockfile
                    self.result.lockfile.islands
                        .insert(island_id, island_result.descriptor_to_locator);
                }

                // Fetch all island-resolved packages so package_data is
                // available for checksum computation and linking.
                let fetch_futures = island_locators.into_iter().map(|locator| {
                    ensure_fetched(locator, false, &self.context, &maps)
                });
                futures::future::join_all(fetch_futures).await;
            } else {
                greedy_future.await;
            }

            Ok::<(), Error>(())
        }).await?;

        // Collect errors from both maps
        let mut errors
            = maps.resolution_map.collect_errors();

        errors.extend(maps.fetch_map.collect_errors());

        if !errors.is_empty() {
            return Err(Error::SilentError);
        }

        let resolution_map
            = Arc::try_unwrap(maps.resolution_map)
                .unwrap_or_else(|_| panic!("resolution_map should have no other references"));

        let fetch_map
            = Arc::try_unwrap(maps.fetch_map)
                .unwrap_or_else(|_| panic!("fetch_map should have no other references"));

        for (descriptor, result) in resolution_map.into_results() {
            let Ok(ResolutionResult { resolution, original_resolution, package_data }) = result else {
                unreachable!("Already handled above")
            };

            self.record_descriptor(descriptor, resolution.locator.clone());
            self.record_resolution(resolution, original_resolution, package_data)?;
        }

        for (locator, result) in fetch_map.into_results() {
            let Ok(FetchResult {package_data, ..}) = result else {
                unreachable!("Already handled above");
            };

            self.record_fetch(locator, package_data)?;
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

        // Build resolution tree. If islands are configured, run a separate
        // TreeResolver per island (isolated peer dep context) and one for
        // greedy-resolved workspaces, then merge the results.
        if resolved_islands.is_some() && !self.result.install_state.island_descriptor_to_locator.is_empty() {
            // Greedy tree: only non-island workspace roots.
            let mut merged_tree = TreeResolver::default()
                .with_resolutions(&self.result.install_state.descriptor_to_locator, &self.result.install_state.normalized_resolutions)?
                .with_roots(self.result.roots.clone())
                .run();

            // Per-island trees (isolated peer dep context per island)
            for (island_id, island_d2l) in &self.result.install_state.island_descriptor_to_locator {
                if island_d2l.is_empty() {
                    continue;
                }

                let island_resolutions = self.result.install_state.island_normalized_resolutions
                    .get(island_id)
                    .cloned()
                    .unwrap_or_default();

                // Island roots: only descriptors that are actually in the island d2l
                let island_roots: BTreeSet<Descriptor> = island_d2l.keys()
                    .cloned()
                    .collect();

                let island_tree = TreeResolver::default()
                    .with_resolutions(island_d2l, &island_resolutions)?
                    .with_roots(island_roots)
                    .run();

                // Merge island tree into the combined tree
                merged_tree.descriptor_to_locator.extend(island_tree.descriptor_to_locator);
                merged_tree.locator_resolutions.extend(island_tree.locator_resolutions);
                merged_tree.optional_builds.extend(island_tree.optional_builds);
                merged_tree.roots.extend(island_tree.roots);
            }

            self.result.install_state.resolution_tree = merged_tree;
        } else {
            self.result.install_state.resolution_tree = TreeResolver::default()
                .with_resolutions(&self.result.install_state.descriptor_to_locator, &self.result.install_state.normalized_resolutions)?
                .with_roots(self.result.roots.clone())
                .run();
        }

        let project
            = self.context.project
                .expect("The project is required to compute workspace hashes");

        self.result.lockfile.resolutions = self.result.install_state.descriptor_to_locator.clone();

        self.result.lockfile.workspaces = self.compute_all_workspace_hashes(&project.workspaces);

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

    fn compute_all_workspace_hashes(&self, workspaces: &[crate::project::Workspace]) -> BTreeMap<Ident, Hash64> {
        let resolutions = &self.result.install_state.normalized_resolutions;
        let d2l = &self.result.install_state.descriptor_to_locator;

        let mut cache: HashMap<Locator, Hash64>
            = HashMap::with_capacity(resolutions.len());
        let mut in_progress: HashSet<Locator>
            = HashSet::new();

        for locator in resolutions.keys() {
            compute_locator_hash(locator, resolutions, d2l, &mut cache, &mut in_progress);
        }

        workspaces.iter()
            .map(|workspace| {
                let root = workspace.locator();
                let hash = cache.get(&root)
                    .cloned()
                    .unwrap_or_else(|| Hash64::from_data(root.to_file_string()));
                (workspace.name.clone(), hash)
            })
            .collect()
    }
}

fn compute_locator_hash(
    locator: &Locator,
    resolutions: &BTreeMap<Locator, Resolution>,
    d2l: &BTreeMap<Descriptor, Locator>,
    cache: &mut HashMap<Locator, Hash64>,
    in_progress: &mut HashSet<Locator>,
) -> Hash64 {
    if let Some(hash) = cache.get(locator) {
        return hash.clone();
    }

    if !in_progress.insert(locator.clone()) {
        return Hash64::from_data(locator.to_file_string());
    }

    let mut hash_writer = Hash64Writer::new();
    hash_writer.update(locator.to_file_string());

    if let Some(resolution) = resolutions.get(locator) {
        let mut child_hashes: Vec<Hash64> = Vec::new();

        for dep_descriptor in resolution.dependencies.values() {
            if let Some(dep_locator) = d2l.get(dep_descriptor) {
                child_hashes.push(compute_locator_hash(dep_locator, resolutions, d2l, cache, in_progress));
            }
        }

        for variant_descriptor in &resolution.variants {
            if let Some(variant_locator) = d2l.get(variant_descriptor) {
                child_hashes.push(compute_locator_hash(variant_locator, resolutions, d2l, cache, in_progress));
            }
        }

        child_hashes.sort();
        for h in &child_hashes {
            hash_writer.update(h.to_file_string());
        }
    }

    let hash = hash_writer.finalize();
    cache.insert(locator.clone(), hash.clone());
    in_progress.remove(locator);
    hash
}

fn normalize_resolution(context: &InstallContext<'_>, descriptor: &mut Descriptor, resolution: &Resolution, apply_overrides: bool) -> Result<(), Error> {
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
        Range::Catalog(params) => {
            let project
                = context.project
                    .expect("The project is required to normalize catalog resolutions");

            descriptor.range
                = lookup_catalog_entry(project, params, &descriptor.ident)?;

            if descriptor.range.details().require_binding {
                descriptor.parent = Some(project.root_workspace().locator());
            } else {
                descriptor.parent = None;
            }

            normalize_resolution(context, descriptor, resolution, false)?;
        },

        Range::Patch(params) => {
            normalize_resolution(context, &mut params.inner.as_mut().0, resolution, false)?;
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

    Ok(())
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

pub fn normalize_resolutions(context: &InstallContext<'_>, resolution: &Resolution) -> Result<(BTreeMap<Ident, Descriptor>, BTreeMap<Ident, PeerRange>), Error> {
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
        normalize_resolution(context, descriptor, resolution, true)?;
    }

    for name in peer_dependencies.keys().filter(|ident| ident.scope() != Some("@types")).cloned().collect::<Vec<_>>() {
        peer_dependencies.entry(name.type_ident())
            .or_insert(SemverPeerRange {range: zpm_semver::Range::from_file_string("*").unwrap()}.into());
    }

    Ok((dependencies, peer_dependencies))
}
