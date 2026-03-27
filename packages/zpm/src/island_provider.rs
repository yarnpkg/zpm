use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fmt;

use pubgrub::{DependencyProvider, Dependencies, PackageResolutionStatistics, VersionSet};
use zpm_primitives::{Descriptor, Ident, Locator, Reference, Registry, RegistryReference};

use crate::error::Error;
use crate::install::InstallContext;
use crate::island_types::{IslandVersion, IslandVersionSet};
use crate::resolvers;
use crate::resolvers::Resolution;

/// Error type for island resolution, compatible with pubgrub's error trait.
#[derive(Clone, Debug)]
pub struct IslandResolutionError {
    pub message: String,
}

impl fmt::Display for IslandResolutionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for IslandResolutionError {}

impl From<Error> for IslandResolutionError {
    fn from(err: Error) -> Self {
        IslandResolutionError {
            message: format!("{}", err),
        }
    }
}

/// Priority for pubgrub package selection.
/// Lower version count = higher priority (finds conflicts faster).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IslandPriority {
    /// Whether this is a locked package (highest priority)
    pub is_locked: bool,
    /// Inverse of matching version count (fewer = higher priority)
    pub inverse_count: u64,
}

impl Ord for IslandPriority {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.is_locked.cmp(&other.is_locked)
            .then(self.inverse_count.cmp(&other.inverse_count))
    }
}

impl PartialOrd for IslandPriority {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// The pubgrub DependencyProvider for island resolution.
pub struct IslandDependencyProvider<'a> {
    pub island_id: String,
    pub locked_versions: BTreeMap<Ident, Locator>,
    pub enforced_resolutions: BTreeMap<Descriptor, Option<Locator>>,
    pub handle: tokio::runtime::Handle,
    /// The root dependencies: one entry per workspace as an exact singleton.
    pub root_deps: BTreeMap<Ident, IslandVersionSet>,
    /// Install context for resolving versions and dependencies from the registry.
    pub ctx: &'a InstallContext<'a>,
    /// Each workspace's manifest dependencies (regular + optionally dev).
    pub workspace_deps: BTreeMap<Ident, BTreeMap<Ident, Descriptor>>,
    /// Cache of full resolutions fetched during dependency resolution.
    /// Keyed by locator so convert_solution can retrieve them.
    pub resolution_cache: RefCell<BTreeMap<Locator, Resolution>>,
}

impl<'a> IslandDependencyProvider<'a> {
    pub fn new(
        island_id: String,
        locked_versions: BTreeMap<Ident, Locator>,
        enforced_resolutions: BTreeMap<Descriptor, Option<Locator>>,
        handle: tokio::runtime::Handle,
        root_deps: BTreeMap<Ident, IslandVersionSet>,
        ctx: &'a InstallContext<'a>,
        workspace_deps: BTreeMap<Ident, BTreeMap<Ident, Descriptor>>,
    ) -> Self {
        Self {
            island_id,
            locked_versions,
            enforced_resolutions,
            handle,
            root_deps,
            ctx,
            workspace_deps,
            resolution_cache: RefCell::new(BTreeMap::new()),
        }
    }

    /// Fetch all available versions for a package from the registry,
    /// returning them as IslandVersions in descending order (newest first).
    fn fetch_versions(&self, package: &Ident) -> Result<Vec<IslandVersion>, IslandResolutionError> {
        let registry = Registry::Npm(package.clone());
        let versions = self.handle.block_on(
            resolvers::resolve_versions(self.ctx, &registry)
        ).map_err(IslandResolutionError::from)?;

        // Reverse so newest versions come first (BTreeMap yields ascending order)
        Ok(versions.into_iter().rev().map(IslandVersion).collect())
    }

    /// Convert a map of dependency descriptors into IslandVersionSets for pubgrub.
    ///
    /// Semver ranges are converted directly. Non-semver ranges are resolved
    /// on the spot via `resolve_descriptor` and injected as exact singletons.
    fn descriptors_to_deps(&self, descriptors: &BTreeMap<Ident, Descriptor>) -> Result<BTreeMap<Ident, IslandVersionSet>, IslandResolutionError> {
        let mut deps = BTreeMap::new();
        let mut non_semver: Vec<(Ident, Descriptor)> = Vec::new();

        for (ident, descriptor) in descriptors {
            match crate::island::range_to_version_set(&descriptor.range) {
                Some(vs) => {
                    deps.insert(ident.clone(), vs);
                }
                None => {
                    non_semver.push((ident.clone(), descriptor.clone()));
                }
            }
        }

        if !non_semver.is_empty() {
            // Resolve all non-semver deps in parallel.
            let futures = non_semver.iter().map(|(_, descriptor)| {
                crate::resolvers::resolve_descriptor(self.ctx.clone(), descriptor.clone(), vec![])
            });

            let results = self.handle.block_on(
                futures::future::try_join_all(futures)
            ).map_err(IslandResolutionError::from)?;

            let mut cache = self.resolution_cache.borrow_mut();
            for ((ident, _), result) in non_semver.into_iter().zip(results) {
                let locator = result.resolution.locator.clone();
                cache.insert(locator.clone(), result.resolution);
                deps.insert(ident, IslandVersionSet::exact_singleton(IslandVersion(locator)));
            }
        }

        Ok(deps)
    }

    /// Convert a Resolution's dependencies into IslandVersionSets for pubgrub.
    fn resolution_to_deps(&self, resolution: &Resolution) -> Result<BTreeMap<Ident, IslandVersionSet>, IslandResolutionError> {
        self.descriptors_to_deps(&resolution.dependencies)
    }

    /// Fetch the dependencies of a specific package version from the registry.
    /// Also caches the full Resolution for later use in convert_solution.
    fn fetch_dependencies(&self, version: &IslandVersion) -> Result<BTreeMap<Ident, IslandVersionSet>, IslandResolutionError> {
        let locator = &version.0;

        // Check resolution cache first — covers non-semver packages resolved
        // lazily in choose_version or resolution_to_deps.
        if let Some(resolution) = self.resolution_cache.borrow().get(locator).cloned() {
            return self.resolution_to_deps(&resolution);
        }

        let params = match &locator.reference {
            Reference::Registry(params) => params.clone(),
            Reference::Shorthand(params) => RegistryReference {
                ident: locator.ident.clone(),
                version: params.version.clone(),
                url: None,
            },
            _ => return Ok(BTreeMap::new()),
        };

        let result = self.handle.block_on(
            crate::resolvers::npm::resolve_locator(self.ctx, locator, &params)
        ).map_err(IslandResolutionError::from)?;

        // Cache the full resolution for convert_solution
        self.resolution_cache.borrow_mut()
            .insert(locator.clone(), result.resolution.clone());

        self.resolution_to_deps(&result.resolution)
    }
}

impl DependencyProvider for IslandDependencyProvider<'_> {
    type P = Ident;
    type V = IslandVersion;
    type VS = IslandVersionSet;
    type Priority = IslandPriority;
    type M = String;
    type Err = IslandResolutionError;

    fn prioritize(
        &self,
        package: &Ident,
        _range: &IslandVersionSet,
        _stats: &PackageResolutionStatistics,
    ) -> IslandPriority {
        let is_locked = self.locked_versions.contains_key(package);

        let inverse_count = if is_locked { u64::MAX } else { 0 };

        IslandPriority {
            is_locked,
            inverse_count,
        }
    }

    fn choose_version(
        &self,
        package: &Ident,
        range: &IslandVersionSet,
    ) -> Result<Option<IslandVersion>, IslandResolutionError> {
        // Virtual root
        if package.as_str() == "__island_root__" {
            let root_locator = Locator::new(
                package.clone(),
                zpm_primitives::ShorthandReference {
                    version: zpm_semver::Version::default(),
                }.into(),
            );
            let root_version = IslandVersion(root_locator);
            if range.contains(&root_version) {
                return Ok(Some(root_version));
            }
            return Ok(None);
        }

        // Exact singletons (workspace packages, non-semver deps from
        // descriptors_to_deps) have exactly one possible version — return it directly.
        if let IslandVersionSet::Exact(crate::island_types::ExactSet::Singleton(v)) = range {
            return Ok(Some(v.clone()));
        }

        // Check locked version first
        if let Some(locked_locator) = self.locked_versions.get(package) {
            let iv = IslandVersion(locked_locator.clone());
            if range.contains(&iv) {
                return Ok(Some(iv));
            }
        }

        // Fetch all available versions from the registry and pick the first
        // (newest) that satisfies the range.
        let versions = self.fetch_versions(package)?;
        for iv in versions {
            if range.contains(&iv) {
                return Ok(Some(iv));
            }
        }

        Ok(None)
    }

    fn get_dependencies(
        &self,
        package: &Ident,
        version: &IslandVersion,
    ) -> Result<Dependencies<Ident, IslandVersionSet, String>, IslandResolutionError> {
        // Virtual root returns the island's workspace dependencies
        if package.as_str() == "__island_root__" {
            let deps: pubgrub::DependencyConstraints<Ident, IslandVersionSet> = self.root_deps.clone()
                .into_iter()
                .collect();
            return Ok(Dependencies::Available(deps));
        }

        // Workspace packages: return their manifest dependencies
        if let Some(ws_deps) = self.workspace_deps.get(package) {
            let deps = self.descriptors_to_deps(ws_deps)?;
            return Ok(Dependencies::Available(deps.into_iter().collect()));
        }

        // Fetch the package manifest and extract its dependencies
        let deps = self.fetch_dependencies(version)?;

        Ok(Dependencies::Available(deps.into_iter().collect()))
    }
}
