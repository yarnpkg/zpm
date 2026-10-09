use std::cell::{Cell, RefCell};
use std::cmp::Reverse;
use std::collections::BTreeMap;
use std::fmt;

use pubgrub::{DependencyProvider, Dependencies, PackageResolutionStatistics, VersionSet};
use zpm_primitives::{Descriptor, Ident, Locator, PypiRegistryReference, PypiSpecifierRange, PypiTagRange, Range, Reference, Registry, RegistryReference};

use crate::error::Error;
use crate::install::InstallContext;
use crate::island_types::{ExactSet, IslandPackage, IslandVersion, IslandVersionSet};
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

/// Priority tiers for pubgrub package selection.
///
/// Derived `Ord` gives higher priority to later variants. Within each
/// tier, `Reverse<usize>` orders by discovery time (earlier = higher).
///
/// Modelled after uv's `PubGrubPriority`:
///   <https://github.com/astral-sh/uv/blob/main/crates/uv-resolver/src/pubgrub/priority.rs>
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum IslandPriority {
    /// Default for range constraints (e.g. `^1.0.0`).
    Unspecified(Reverse<usize>),
    /// Packages involved in many conflicts — resolve earlier to prune
    /// the search space.
    Conflict(Reverse<usize>),
    /// Packages pinned to a single version (exact semver or non-semver
    /// singleton). Only one possible choice, so free to resolve first.
    Singleton(Reverse<usize>),
    /// Packages whose version is locked in the previous lockfile.
    Locked(Reverse<usize>),
}

/// The pubgrub DependencyProvider for island resolution.
pub struct IslandDependencyProvider<'a> {
    pub island_id: String,
    pub locked_versions: BTreeMap<Ident, Locator>,
    pub enforced_resolutions: BTreeMap<Descriptor, Option<Locator>>,
    pub handle: tokio::runtime::Handle,
    /// The root dependencies: one entry per workspace as an exact singleton.
    pub root_deps: BTreeMap<IslandPackage, IslandVersionSet>,
    /// Install context for resolving versions and dependencies from the registry.
    pub ctx: &'a InstallContext<'a>,
    /// Each workspace's manifest dependencies (regular + optionally dev).
    pub workspace_deps: BTreeMap<Ident, BTreeMap<Ident, Descriptor>>,
    /// Cache of full resolutions fetched during dependency resolution.
    /// Keyed by locator so convert_solution can retrieve them.
    pub resolution_cache: RefCell<BTreeMap<Locator, Resolution>>,
    /// Cache of extra-specific dependency resolutions. These are solver
    /// proxies only: their dependencies are merged back into the base package
    /// resolution after PubGrub completes.
    pub extra_resolution_cache: RefCell<BTreeMap<(Locator, String), Resolution>>,
    /// Monotonic counter for discovery order (breadth-first priority).
    discovery_counter: Cell<usize>,
    /// Maps package ident → discovery index (first time seen in prioritize).
    discovery_order: RefCell<BTreeMap<Ident, usize>>,
    /// Packages required through `pypi:` ranges; their candidates come
    /// from the PyPI index rather than the npm registry.
    pub pypi_idents: RefCell<std::collections::BTreeSet<Ident>>,
    /// Memoized PyPI candidates (highest first).
    pypi_candidates: RefCell<BTreeMap<Ident, Vec<IslandVersion>>>,
    /// Memoized `local_workspaces()`
    local_workspaces: RefCell<Option<std::collections::BTreeSet<Ident>>>,
}

impl<'a> IslandDependencyProvider<'a> {
    pub fn new(
        island_id: String,
        locked_versions: BTreeMap<Ident, Locator>,
        enforced_resolutions: BTreeMap<Descriptor, Option<Locator>>,
        handle: tokio::runtime::Handle,
        root_deps: BTreeMap<IslandPackage, IslandVersionSet>,
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
            extra_resolution_cache: RefCell::new(BTreeMap::new()),
            discovery_counter: Cell::new(0),
            discovery_order: RefCell::new(BTreeMap::new()),
            pypi_idents: RefCell::new(std::collections::BTreeSet::new()),
            pypi_candidates: RefCell::new(BTreeMap::new()),
            local_workspaces: RefCell::new(None),
        }
    }

    /// Candidate versions of a PyPI package, highest first.
    fn satisfies_constraints(&self, ident: &Ident, version: &IslandVersion) -> bool {
        let pypi_version = match &version.0.reference {
            zpm_primitives::Reference::PypiShorthand(params) if params.url.is_none() => &params.version,
            zpm_primitives::Reference::PypiRegistry(params) if params.url.is_none() => &params.version,
            _ => return true,
        };

        crate::resolvers::pypi::satisfies_constraints(self.ctx, ident, pypi_version)
    }

    fn fetch_pypi_versions(&self, package: &Ident) -> Result<Vec<IslandVersion>, IslandResolutionError> {
        if let Some(versions) = self.pypi_candidates.borrow().get(package) {
            return Ok(versions.clone());
        }

        let versions = self.handle.block_on(
            crate::resolvers::pypi::candidate_versions(self.ctx, package)
        ).map_err(IslandResolutionError::from)?;

        let versions = versions.into_iter()
            .map(|version| IslandVersion(Locator::new(package.clone(), zpm_primitives::PypiShorthandReference {
                version,
                url: None,
            }.into())))
            .collect::<Vec<_>>();

        self.pypi_candidates.borrow_mut()
            .insert(package.clone(), versions.clone());

        Ok(versions)
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
    /// The workspace locator for `ident` when the island already uses that
    /// workspace through a local (`workspace:`) edge somewhere in its graph.
    fn python_workspace_locator(&self, ident: &Ident) -> Option<Locator> {
        if !self.local_workspaces().contains(ident) {
            return None;
        }

        Some(Locator::new(ident.clone(), zpm_primitives::WorkspaceIdentReference {ident: ident.clone()}.into()))
    }

    /// The workspaces reachable from the island's own workspaces through
    /// `workspace:` dependencies (path sources in uv terms)
    fn local_workspaces(&self) -> std::cell::Ref<'_, std::collections::BTreeSet<Ident>> {
        if self.local_workspaces.borrow().is_none() {
            let mut seen
                = std::collections::BTreeSet::new();

            let mut queue
                = self.workspace_deps.keys().cloned().collect::<Vec<_>>();

            while let Some(ident) = queue.pop() {
                if !seen.insert(ident.clone()) {
                    continue;
                }

                let dependencies = match self.workspace_deps.get(&ident) {
                    Some(dependencies) => dependencies.clone(),
                    None => self.ctx.project
                        .and_then(|project| project.workspace_by_ident(&ident).ok())
                        .map(|workspace| workspace.manifest.remote.dependencies.clone())
                        .unwrap_or_default(),
                };

                for (dependency_ident, descriptor) in dependencies {
                    if descriptor.range.is_workspace() {
                        queue.push(dependency_ident);
                    }
                }
            }

            *self.local_workspaces.borrow_mut() = Some(seen);
        }

        std::cell::Ref::map(self.local_workspaces.borrow(), |set| set.as_ref().unwrap())
    }

    /// Registry packages get the island's overrides applied when they're
    /// resolved; workspaces are read straight from their manifests, so the
    /// overrides are applied here. Like uv's override-dependencies, they
    /// replace the requirements of local projects too (for example the range
    /// a path dependency declares).
    fn apply_overrides(&self, locator: &Locator, descriptors: &BTreeMap<Ident, Descriptor>) -> BTreeMap<Ident, Descriptor> {
        let mut resolution
            = crate::resolvers::Resolution::new_empty(locator.clone(), zpm_semver::Version::new());

        resolution.dependencies = descriptors.clone();

        match crate::install::normalize_resolutions(&self.ctx, &resolution) {
            Ok((dependencies, _)) => dependencies,
            Err(_) => descriptors.clone(),
        }
    }

    fn descriptors_to_deps(&self, descriptors: &BTreeMap<Ident, Descriptor>) -> Result<BTreeMap<IslandPackage, IslandVersionSet>, IslandResolutionError> {
        let mut deps = BTreeMap::new();
        let mut non_semver: Vec<(Ident, Descriptor, Vec<String>)> = Vec::new();

        for (ident, descriptor) in descriptors {
            let extras = pypi_extras(&descriptor.range);

            // A PyPI requirement on a package that's a Python workspace of
            // the project resolves to the workspace, whatever its range: uv
            // does the same once a project gets the package from a local
            // source (a project requiring `lib>=3.10` while a plugin brings
            // the local `lib`)
            if matches!(descriptor.range, Range::PypiSpecifier(_)) {
                if let Some(workspace_locator) = self.python_workspace_locator(ident) {
                    insert_dependency_packages(&mut deps, ident, extras, IslandVersionSet::exact_singleton(IslandVersion(workspace_locator)));
                    continue;
                }
            }

            if let Some(vs) = pypi_version_set(&descriptor.range) {
                self.pypi_idents.borrow_mut().insert(ident.clone());
                insert_dependency_packages(&mut deps, ident, extras, vs);
                continue;
            }

            let descriptor = descriptor_without_pypi_extras(descriptor);

            match crate::island::range_to_version_set(&descriptor.range) {
                Some(vs) => {
                    insert_dependency_packages(&mut deps, ident, extras, vs);
                }
                None => {
                    non_semver.push((ident.clone(), descriptor, extras));
                }
            }
        }

        if !non_semver.is_empty() {
            // Resolve all non-semver deps in parallel.
            let futures = non_semver.iter().map(|(_, descriptor, _)| {
                crate::resolvers::resolve_descriptor(self.ctx.clone(), descriptor.clone(), vec![])
            });

            let results = self.handle.block_on(
                futures::future::try_join_all(futures)
            ).map_err(IslandResolutionError::from)?;

            for ((ident, _, extras), result) in non_semver.into_iter().zip(results) {
                let locator = result.resolution.locator.clone();

                self.resolution_cache.borrow_mut()
                    .insert(locator.clone(), result.resolution);

                insert_dependency_packages(
                    &mut deps,
                    &ident,
                    extras,
                    IslandVersionSet::exact_singleton(IslandVersion(locator)),
                );
            }
        }

        Ok(deps)
    }

    /// Convert a Resolution's dependencies into IslandVersionSets for pubgrub.
    fn resolution_to_deps(&self, resolution: &Resolution) -> Result<BTreeMap<IslandPackage, IslandVersionSet>, IslandResolutionError> {
        self.descriptors_to_deps(&resolution.dependencies)
    }

    /// Fetch the dependencies of a specific package version from the registry.
    /// Also caches the full Resolution for later use in convert_solution.
    fn fetch_dependencies(&self, version: &IslandVersion) -> Result<BTreeMap<IslandPackage, IslandVersionSet>, IslandResolutionError> {
        let locator = &version.0;

        // Check resolution cache first — covers non-semver packages resolved
        // lazily in choose_version or resolution_to_deps.
        let cached_resolution = {
            let cache = self.resolution_cache.borrow();
            cache.get(locator).cloned()
        };

        if let Some(resolution) = cached_resolution {
            return self.resolution_to_deps(&resolution);
        }

        let result = match &locator.reference {
            Reference::Registry(params) => {
                self.handle.block_on(
                    crate::resolvers::npm::resolve_locator(self.ctx, locator, params)
                ).map_err(IslandResolutionError::from)?
            },

            Reference::Shorthand(params) => {
                let params = RegistryReference {
                    ident: locator.ident.clone(),
                    version: params.version.clone(),
                    url: None,
                };

                self.handle.block_on(
                    crate::resolvers::npm::resolve_locator(self.ctx, locator, &params)
                ).map_err(IslandResolutionError::from)?
            },

            Reference::PypiRegistry(params) => {
                self.handle.block_on(
                    crate::resolvers::pypi::resolve_locator(self.ctx, locator, params)
                ).map_err(IslandResolutionError::from)?
            },

            Reference::PypiShorthand(params) => {
                let params = PypiRegistryReference {
                    ident: locator.ident.clone(),
                    version: params.version.clone(),
                    url: params.url.clone(),
                };

                self.handle.block_on(
                    crate::resolvers::pypi::resolve_locator(self.ctx, locator, &params)
                ).map_err(IslandResolutionError::from)?
            },

            _ => return Ok(BTreeMap::new()),
        };

        // Cache the full resolution for convert_solution
        self.resolution_cache.borrow_mut()
            .insert(locator.clone(), result.resolution.clone());

        self.resolution_to_deps(&result.resolution)
    }

    fn fetch_extra_dependencies(&self, version: &IslandVersion, extra: &str) -> Result<BTreeMap<IslandPackage, IslandVersionSet>, IslandResolutionError> {
        let locator = &version.0;
        let cache_key = (locator.clone(), extra.to_string());

        let cached_resolution = {
            let cache = self.extra_resolution_cache.borrow();
            cache.get(&cache_key).cloned()
        };

        if let Some(resolution) = cached_resolution {
            return self.resolution_to_deps(&resolution);
        }

        let result = match &locator.reference {
            Reference::PypiRegistry(params) => {
                self.handle.block_on(
                    crate::resolvers::pypi::resolve_locator_extra(self.ctx, locator, params, extra)
                ).map_err(IslandResolutionError::from)?
            },

            Reference::PypiShorthand(params) => {
                let params = PypiRegistryReference {
                    ident: locator.ident.clone(),
                    version: params.version.clone(),
                    url: params.url.clone(),
                };

                self.handle.block_on(
                    crate::resolvers::pypi::resolve_locator_extra(self.ctx, locator, &params, extra)
                ).map_err(IslandResolutionError::from)?
            },

            _ => return Ok(BTreeMap::new()),
        };

        self.extra_resolution_cache.borrow_mut()
            .insert(cache_key, result.resolution.clone());

        self.resolution_to_deps(&result.resolution)
    }
}

/// PyPI specifier ranges (without alias nor platform selector) are solved
/// natively, as PEP 440 version ranges.
fn pypi_version_set(range: &Range) -> Option<IslandVersionSet> {
    let Range::PypiSpecifier(params) = range else {
        return None;
    };

    if params.ident.is_some() || params.parameters.as_ref().map_or(false, |parameters| parameters.platform.is_some()) {
        return None;
    }

    IslandVersionSet::from_pypi_specifier(&params.specifier)
}

fn pypi_extras(range: &Range) -> Vec<String> {
    match range {
        Range::PypiSpecifier(params) => params.parameters.iter()
            .flat_map(|parameters| parameters.extras.iter())
            .flat_map(|extras| extras.iter().map(|extra| extra.to_string()))
            .collect(),

        Range::PypiTag(params) => params.parameters.iter()
            .flat_map(|parameters| parameters.extras.iter())
            .flat_map(|extras| extras.iter().map(|extra| extra.to_string()))
            .collect(),

        _ => Vec::new(),
    }
}

fn descriptor_without_pypi_extras(descriptor: &Descriptor) -> Descriptor {
    let range = match &descriptor.range {
        Range::PypiSpecifier(params) if params.parameters.as_ref().and_then(|parameters| parameters.extras.as_ref()).is_some() => {
            Range::PypiSpecifier(PypiSpecifierRange {
                ident: params.ident.clone(),
                specifier: params.specifier.clone(),
                parameters: None,
            })
        }

        Range::PypiTag(params) if params.parameters.as_ref().and_then(|parameters| parameters.extras.as_ref()).is_some() => {
            Range::PypiTag(PypiTagRange {
                ident: params.ident.clone(),
                tag: params.tag.clone(),
                parameters: None,
            })
        }

        _ => descriptor.range.clone(),
    };

    Descriptor::new_bound(descriptor.ident.clone(), range, descriptor.parent.clone())
}

fn insert_dependency_packages(
    deps: &mut BTreeMap<IslandPackage, IslandVersionSet>,
    ident: &Ident,
    extras: Vec<String>,
    version_set: IslandVersionSet,
) {
    if extras.is_empty() {
        deps.insert(IslandPackage::Named(ident.clone()), version_set);
    } else {
        for extra in extras {
            deps.insert(
                IslandPackage::ExtraProxy {
                    ident: ident.clone(),
                    extra,
                },
                version_set.clone(),
            );
        }
    }
}

impl DependencyProvider for IslandDependencyProvider<'_> {
    type P = IslandPackage;
    type V = IslandVersion;
    type VS = IslandVersionSet;
    type Priority = IslandPriority;
    type M = String;
    type Err = IslandResolutionError;

    fn prioritize(
        &self,
        package: &IslandPackage,
        range: &IslandVersionSet,
        stats: &PackageResolutionStatistics,
    ) -> IslandPriority {
        let Some(ident) = package.ident() else {
            return IslandPriority::Locked(Reverse(0));
        };

        // Assign a stable discovery index (first time we see this package).
        let discovery_idx = {
            let mut order = self.discovery_order.borrow_mut();
            let len = order.len();
            *order.entry(ident.clone()).or_insert_with(|| {
                let idx = self.discovery_counter.get();
                self.discovery_counter.set(idx + 1);
                let _ = len; // suppress unused
                idx
            })
        };

        if self.locked_versions.contains_key(ident) {
            return IslandPriority::Locked(Reverse(discovery_idx));
        }

        // Detect singleton constraints: exact non-semver (OneOf with one
        // element) or a semver range that matches exactly one version.
        let is_singleton = match range {
            IslandVersionSet::Exact(ExactSet::OneOf(vs)) => vs.len() == 1,
            IslandVersionSet::Semver(r) => r.as_singleton().is_some(),
            IslandVersionSet::Pypi(r) => r.as_singleton().is_some(),
            _ => false,
        };

        if is_singleton {
            return IslandPriority::Singleton(Reverse(discovery_idx));
        }

        // Promote packages involved in conflicts so they are resolved
        // earlier on subsequent passes, pruning the search space faster.
        if stats.conflict_count() > 0 {
            return IslandPriority::Conflict(Reverse(discovery_idx));
        }

        IslandPriority::Unspecified(Reverse(discovery_idx))
    }

    fn choose_version(
        &self,
        package: &IslandPackage,
        range: &IslandVersionSet,
    ) -> Result<Option<IslandVersion>, IslandResolutionError> {
        let ident = match package {
            IslandPackage::Root => {
                let root_locator = Locator::new(
                    Ident::default(),
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
            IslandPackage::Named(ident)
            | IslandPackage::ExtraProxy { ident, .. }
            | IslandPackage::ExtraFeature { ident, .. } => ident,
        };

        // Exact singletons (workspace packages, non-semver deps from
        // descriptors_to_deps) have exactly one possible version — return it directly.
        if let IslandVersionSet::Exact(crate::island_types::ExactSet::OneOf(vs)) = range {
            if vs.len() == 1 {
                return Ok(Some(vs[0].clone()));
            }
        }

        // Check locked version first; constraints added since it got locked
        // (uv's constraint-dependencies) may exclude it
        if let Some(locked_locator) = self.locked_versions.get(ident) {
            let iv = IslandVersion(locked_locator.clone());
            if range.contains(&iv) && self.satisfies_constraints(ident, &iv) {
                return Ok(Some(iv));
            }
        }

        let is_pypi
            = matches!(range, IslandVersionSet::Pypi(_)) || self.pypi_idents.borrow().contains(ident);

        if is_pypi {
            let versions = self.fetch_pypi_versions(ident)?;

            // Pre-releases are only picked when no final release matches
            let stable = versions.iter()
                .find(|iv| iv.pypi_version().map_or(false, |version| !version.any_prerelease()) && range.contains(iv));

            let any = stable.or_else(|| versions.iter().find(|iv| range.contains(iv)));

            return Ok(any.cloned());
        }

        // Fetch all available versions from the registry and pick the first
        // (newest) that satisfies the range.
        let versions = self.fetch_versions(ident)?;
        for iv in versions {
            if range.contains(&iv) {
                return Ok(Some(iv));
            }
        }

        Ok(None)
    }

    fn get_dependencies(
        &self,
        package: &IslandPackage,
        version: &IslandVersion,
    ) -> Result<Dependencies<IslandPackage, IslandVersionSet, String>, IslandResolutionError> {
        let ident = match package {
            IslandPackage::Root => {
                let deps: pubgrub::DependencyConstraints<IslandPackage, IslandVersionSet> = self.root_deps.clone()
                    .into_iter()
                    .collect();
                return Ok(Dependencies::Available(deps));
            }
            IslandPackage::Named(ident) => ident,

            IslandPackage::ExtraProxy { ident, extra } => {
                let deps = BTreeMap::from([
                    (
                        IslandPackage::Named(ident.clone()),
                        <IslandVersionSet as VersionSet>::singleton(version.clone()),
                    ),
                    (
                        IslandPackage::ExtraFeature {
                            ident: ident.clone(),
                            extra: extra.clone(),
                        },
                        <IslandVersionSet as VersionSet>::singleton(version.clone()),
                    ),
                ]);

                return Ok(Dependencies::Available(deps.into_iter().collect()));
            }

            IslandPackage::ExtraFeature { extra, .. } => {
                let deps = self.fetch_extra_dependencies(version, extra)?;
                return Ok(Dependencies::Available(deps.into_iter().collect()));
            }
        };

        // Workspace packages: return their manifest dependencies
        if let Some(ws_deps) = self.workspace_deps.get(ident) {
            let deps = self.descriptors_to_deps(&self.apply_overrides(&version.0, ws_deps))?;
            return Ok(Dependencies::Available(deps.into_iter().collect()));
        }

        // Workspaces from other islands (path dependencies between Python
        // projects) only contribute their regular dependencies, never their
        // devDependencies - same as uv's editable path dependencies.
        if version.0.reference.is_workspace_reference() {
            if let Some(project) = self.ctx.project {
                if let Ok(workspace) = project.workspace_by_ident(ident) {
                    let deps = self.descriptors_to_deps(&self.apply_overrides(&version.0, &workspace.manifest.remote.dependencies))?;
                    return Ok(Dependencies::Available(deps.into_iter().collect()));
                }
            }
        }

        // Fetch the package manifest and extract its dependencies
        let deps = self.fetch_dependencies(version)?;

        Ok(Dependencies::Available(deps.into_iter().collect()))
    }
}
