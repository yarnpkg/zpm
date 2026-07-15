use std::collections::{BTreeMap, BTreeSet};

use pubgrub::Reporter;
use zpm_primitives::{Descriptor, Ident, Locator, Reference, ShorthandReference, WorkspaceIdentReference};
use zpm_utils::FromFileString;

use crate::error::Error;
use crate::install::InstallContext;
use crate::island_provider::IslandDependencyProvider;
use crate::island_types::{IslandPackage, IslandVersion, IslandVersionSet};
use crate::lockfile::Lockfile;
use crate::project::Workspace;
use crate::resolvers::Resolution;

/// A resolved island: config globs have been evaluated against the
/// project's actual workspaces to produce the concrete set of workspace
/// idents and their root descriptors.
#[derive(Clone, Debug)]
pub struct ResolvedIsland {
    pub id: String,
    pub workspace_idents: BTreeSet<Ident>,
    pub root_descriptors: BTreeSet<Descriptor>,
    pub linker: zpm_config::IslandLinker,
}

/// The result of resolving an island's dependency graph.
#[derive(Clone, Debug)]
pub struct IslandResolutionResult {
    pub island_id: String,
    pub descriptor_to_locator: BTreeMap<Descriptor, Locator>,
    pub normalized_resolutions: BTreeMap<Locator, Resolution>,
}

/// Build resolved islands from config settings + project workspaces.
/// Validates that no workspace belongs to more than one island and
/// that no island is empty.
pub fn resolve_islands(
    config_islands: &BTreeMap<String, zpm_config::IslandDefinition>,
    workspaces: &[Workspace],
) -> Result<Vec<ResolvedIsland>, Error> {
    let mut resolved = Vec::new();
    let mut workspace_to_island: BTreeMap<Ident, String> = BTreeMap::new();

    for (island_id, island_def) in config_islands {
        let mut workspace_idents = BTreeSet::new();
        let mut root_descriptors = BTreeSet::new();

        for workspace in workspaces {
            let matches = island_def.workspaces.iter().any(|glob| glob.value.check(&workspace.name));

            if matches {
                // Check for duplicate membership
                if let Some(existing_island) = workspace_to_island.get(&workspace.name) {
                    return Err(Error::WorkspaceInMultipleIslands {
                        ident: workspace.name.clone(),
                        islands: vec![existing_island.clone(), island_id.clone()],
                    });
                }

                workspace_to_island.insert(workspace.name.clone(), island_id.clone());
                workspace_idents.insert(workspace.name.clone());
                root_descriptors.insert(workspace.descriptor());
            }
        }

        if workspace_idents.is_empty() {
            return Err(Error::EmptyIsland(island_id.clone()));
        }

        resolved.push(ResolvedIsland {
            id: island_id.clone(),
            workspace_idents,
            root_descriptors,
            linker: island_def.linker.value,
        });
    }

    Ok(resolved)
}

/// Resolve a single island's dependency graph.
///
/// Two-phase strategy:
/// 1. Try lockfile: if all transitive deps are present, reuse them.
/// 2. Run pubgrub with on-demand metadata fetching, using locked
///    versions as preferred.
pub async fn resolve_island(
    island: &ResolvedIsland,
    ctx: &InstallContext<'_>,
    lockfile: &Lockfile,
) -> Result<IslandResolutionResult, Error> {
    // Phase 2: Build locked_versions map (preferred locators from lockfile)
    let locked_versions = build_locked_versions(island, lockfile);

    // Phase 3: Build root_deps (workspace singletons) and workspace_deps
    let project = ctx.project
        .expect("Project is required for island resolution");

    let mut root_deps: BTreeMap<IslandPackage, IslandVersionSet> = BTreeMap::new();
    let mut workspace_deps: BTreeMap<Ident, BTreeMap<Ident, Descriptor>> = BTreeMap::new();

    for workspace in &project.workspaces {
        if !island.workspace_idents.contains(&workspace.name) {
            continue;
        }

        // Create a workspace locator using WorkspaceIdentReference
        let ws_locator = Locator::new(
            workspace.name.clone(),
            WorkspaceIdentReference {
                ident: workspace.name.clone(),
            }.into(),
        );
        let ws_version = IslandVersion(ws_locator);

        // Root depends on each workspace as an exact singleton
        root_deps.insert(
            IslandPackage::Named(workspace.name.clone()),
            IslandVersionSet::exact_singleton(ws_version),
        );

        // Collect this workspace's dependencies for the provider
        let mut deps: BTreeMap<Ident, Descriptor> = BTreeMap::new();

        for (ident, descriptor) in &workspace.manifest.remote.dependencies {
            deps.insert(ident.clone(), descriptor.clone());
        }

        if !ctx.prune_dev_dependencies {
            for (ident, descriptor) in &workspace.manifest.dev_dependencies {
                deps.insert(ident.clone(), descriptor.clone());
            }
        }

        workspace_deps.insert(workspace.name.clone(), deps);
    }

    // TODO: Phase 1 — lockfile fast-path. Currently disabled; always
    // re-resolve to avoid stale lockfile issues when deps change.
    // Future optimisation: compare root_deps against locked island
    // descriptors and reuse the lockfile when they match exactly.

    // Phase 4: Run pubgrub on a blocking thread.
    //
    // We use spawn_blocking because pubgrub::resolve is synchronous and may
    // call handle.block_on() internally (via the provider) to fetch registry
    // data.  The provider holds references to `ctx` which has a non-'static
    // lifetime, so we transmute it to 'static for the spawn_blocking closure.
    //
    // SAFETY: the JoinHandle is `.await`ed immediately below, so the closure
    // always completes before this function returns — the references in `ctx`
    // remain valid for the entire duration of the blocking task.
    let handle = tokio::runtime::Handle::current();
    let island_id = island.id.clone();
    let enforced_resolutions = ctx.enforced_resolutions.clone();

    // SAFETY: see comment above — the references are valid for the lifetime
    // of the spawn_blocking task because we .await the result immediately.
    let ctx_static: &'static InstallContext<'static> = unsafe {
        std::mem::transmute::<&InstallContext<'_>, &'static InstallContext<'static>>(ctx)
    };

    let workspace_deps_for_provider = workspace_deps.clone();

    let (solution, resolution_cache, extra_resolution_cache) = tokio::task::spawn_blocking(move || {
        let provider = IslandDependencyProvider::new(
            island_id.clone(),
            locked_versions,
            enforced_resolutions,
            handle,
            root_deps,
            ctx_static,
            workspace_deps_for_provider,
        );

        let root_locator = Locator::new(
            Ident::default(),
            zpm_primitives::ShorthandReference {
                version: zpm_semver::Version::default(),
            }.into(),
        );
        let root_version = IslandVersion(root_locator);

        // TODO: Drive pubgrub step-by-step (unit_propagation / pick_highest_priority_pkg /
        // add_decision) instead of using the one-shot resolve() API. This would allow:
        //   1. Concurrent metadata prefetching (fetch next packages while solving the current one)
        //   2. ConflictEarly/ConflictLate split (track affected vs culprit separately)
        //   3. Remove the unsafe transmute to 'static (the async fetch loop would own the data)
        // See uv's resolver for reference:
        //   https://github.com/astral-sh/uv/blob/main/crates/uv-resolver/src/resolver/mod.rs
        let result = pubgrub::resolve(&provider, IslandPackage::Root, root_version)
            .map_err(|e| handle_pubgrub_error(&island_id, e));

        // Extract cached resolutions before provider is dropped
        let cache = provider.resolution_cache.into_inner();
        let extra_cache = provider.extra_resolution_cache.into_inner();

        result.map(|solution| (solution, cache, extra_cache))
    }).await.map_err(|e| Error::IslandResolutionFailed {
        island_id: island.id.clone(),
        message: format!("Join error: {}", e),
    })??;

    // Phase 5: Convert pubgrub solution to descriptor_to_locator + resolutions
    convert_solution(&island.id, solution, &resolution_cache, &extra_resolution_cache, &workspace_deps)
}

#[allow(dead_code)]
/// Check if the lockfile has a valid and complete resolution for this island.
/// Validates both that all entries exist and that the locked descriptors
/// match the current workspace dependencies.
fn is_island_lockfile_valid(
    locked_island: &BTreeMap<Descriptor, Locator>,
    lockfile: &Lockfile,
    current_root_deps: &BTreeMap<Ident, IslandVersionSet>,
) -> bool {
    if locked_island.is_empty() && current_root_deps.is_empty() {
        return true;
    }

    if locked_island.is_empty() {
        return false;
    }

    // Verify all locked entries are present in the lockfile
    for locator in locked_island.values() {
        if !lockfile.entries.contains_key(locator) {
            return false;
        }
    }

    // Verify the locked island covers exactly the current root deps.
    // If a dep was added or removed, re-resolve.
    let locked_root_idents: BTreeSet<&Ident> = locked_island.keys()
        .map(|d| &d.ident)
        .collect();

    let current_idents: BTreeSet<&Ident> = current_root_deps.keys()
        .collect();

    // Every current dep must be present, and the locked island shouldn't
    // have root idents that are no longer requested. We use subset checks
    // in both directions — but locked_root_idents includes transitive deps
    // too, so we only check that current is a subset and that removing a
    // current dep invalidates the lockfile.
    if !current_idents.is_subset(&locked_root_idents) {
        // A new dependency was added
        return false;
    }

    // Check that there are no locked root idents that are not in current
    // deps or in transitive deps. Since we can't easily distinguish root
    // from transitive in the lockfile, we use a simpler heuristic: if
    // current_root_deps is empty but locked_island is not, it's stale.
    if current_root_deps.is_empty() && !locked_island.is_empty() {
        return false;
    }

    true
}

#[allow(dead_code)]
/// Build an IslandResolutionResult from cached lockfile data.
fn island_result_from_lockfile(
    island_id: &str,
    locked_island: &BTreeMap<Descriptor, Locator>,
    lockfile: &Lockfile,
) -> Result<IslandResolutionResult, Error> {
    let mut normalized_resolutions = BTreeMap::new();

    for locator in locked_island.values() {
        if let Some(entry) = lockfile.entries.get(locator) {
            normalized_resolutions.insert(locator.clone(), entry.resolution.clone());
        }
    }

    Ok(IslandResolutionResult {
        island_id: island_id.to_string(),
        descriptor_to_locator: locked_island.clone(),
        normalized_resolutions,
    })
}

/// Extract locked locators from the previous lockfile's island data.
fn build_locked_versions(
    island: &ResolvedIsland,
    lockfile: &Lockfile,
) -> BTreeMap<Ident, Locator> {
    let mut locked = BTreeMap::new();

    if let Some(locked_island) = lockfile.islands.get(&island.id) {
        for locator in locked_island.values() {
            locked.entry(locator.ident.clone()).or_insert_with(|| locator.clone());
        }
    }

    locked
}

/// Convert a semver Range to an IslandVersionSet.
///
/// Returns `Some` for semver ranges (AnonymousSemver, RegistrySemver) and
/// `None` for all other range types. Non-semver ranges are handled by
/// pre-resolving them via `resolve_descriptor` before entering pubgrub.
pub fn range_to_version_set(range: &zpm_primitives::Range) -> Option<IslandVersionSet> {
    match range {
        zpm_primitives::Range::AnonymousSemver(params) => {
            Some(IslandVersionSet::from_semver_range(&params.range))
        }
        zpm_primitives::Range::RegistrySemver(params) => {
            Some(IslandVersionSet::from_semver_range(&params.range))
        }
        _ => None,
    }
}

/// Convert pubgrub solution into the ZPM resolution data structures.
fn convert_solution(
    island_id: &str,
    solution: pubgrub::SelectedDependencies<IslandDependencyProvider<'_>>,
    resolution_cache: &BTreeMap<Locator, Resolution>,
    extra_resolution_cache: &BTreeMap<(Locator, String), Resolution>,
    workspace_deps: &BTreeMap<Ident, BTreeMap<Ident, Descriptor>>,
) -> Result<IslandResolutionResult, Error> {
    let mut descriptor_to_locator = BTreeMap::new();
    let mut normalized_resolutions = BTreeMap::new();
    let mut ident_to_locator = BTreeMap::new();
    let mut extra_resolutions = Vec::new();

    for (package, island_version) in &solution {
        // Skip the virtual root
        let ident = match package {
            IslandPackage::Root => continue,
            IslandPackage::Named(ident) => ident,
            IslandPackage::ExtraProxy { .. } => continue,
            IslandPackage::ExtraFeature { ident, extra } => {
                let raw_locator = island_version.0.clone();

                if let Some(resolution) = extra_resolution_cache.get(&(raw_locator.clone(), extra.clone())) {
                    extra_resolutions.push((ident.clone(), raw_locator, resolution.clone()));
                }

                continue;
            }
        };

        // Workspace packages: include them with their dependencies so the
        // tree resolver (and later the WorkTree) can look them up.
        if island_version.0.reference.is_workspace_reference() {
            let locator = island_version.0.clone();
            let descriptor = Descriptor::new(ident.clone(), zpm_primitives::WorkspaceMagicRange {
                magic: zpm_semver::RangeKind::Caret,
            }.into());

            let mut resolution = Resolution::new_empty(locator.clone(), zpm_semver::Version::default());

            // Populate the workspace resolution's dependencies from the
            // workspace manifest deps that were passed to the provider.
            if let Some(deps) = workspace_deps.get(ident) {
                resolution.dependencies = deps.clone();
            }

            ident_to_locator.insert(ident.clone(), locator.clone());
            descriptor_to_locator.insert(descriptor, locator.clone());
            normalized_resolutions.insert(locator, resolution);
            continue;
        }

        let raw_locator = island_version.0.clone();

        // Extract semver version from npm references, if applicable.
        let npm_version = match &raw_locator.reference {
            Reference::Shorthand(params) => Some(params.version.clone()),
            Reference::Registry(params) => Some(params.version.clone()),
            _ => None,
        };

        let (locator, descriptor, version) = if let Some(version) = npm_version {
            // npm packages: normalize RegistryReference → ShorthandReference
            // and create a semver descriptor.
            let locator = Locator::new(ident.clone(), ShorthandReference {
                version: version.clone(),
            }.into());

            let descriptor = Descriptor::new_semver(ident.clone(), &format!("npm:{}", zpm_utils::ToFileString::to_file_string(&version)))
                .unwrap_or_else(|_| {
                    Descriptor::new(ident.clone(), zpm_primitives::AnonymousSemverRange {
                        range: zpm_semver::Range::exact(version.clone()),
                    }.into())
                });

            (locator, descriptor, version)
        } else {
            // Non-npm packages (link, portal, folder, tarball, url, etc.):
            // use the locator as-is and create a descriptor from the
            // reference's file string representation.
            let locator = raw_locator.clone();

            let range_str = zpm_utils::ToFileString::to_file_string(&locator.reference);
            let range = zpm_primitives::Range::from_file_string(&range_str)
                .unwrap_or_else(|_| zpm_primitives::AnonymousSemverRange {
                    range: zpm_semver::Range::any(),
                }.into());
            let descriptor = Descriptor::new(ident.clone(), range);

            (locator, descriptor, zpm_semver::Version::default())
        };

        // Use cached resolution from the provider if available,
        // otherwise create an empty one as fallback.
        let resolution = resolution_cache.get(&raw_locator)
            .cloned()
            .map(|mut res| {
                // Normalize the locator in the cached resolution
                res.locator = locator.clone();
                res
            })
            .unwrap_or_else(|| Resolution::new_empty(locator.clone(), version));

        ident_to_locator.insert(ident.clone(), locator.clone());
        descriptor_to_locator.insert(descriptor, locator.clone());
        normalized_resolutions.insert(locator, resolution);
    }

    for (ident, _, extra_resolution) in extra_resolutions {
        if let Some(base_locator) = ident_to_locator.get(&ident) {
            if let Some(base_resolution) = normalized_resolutions.get_mut(base_locator) {
                for (dep_ident, extra_descriptor) in extra_resolution.dependencies {
                    match base_resolution.dependencies.get_mut(&dep_ident) {
                        Some(base_descriptor) => {
                            crate::resolvers::pypi::merge_dependency_descriptor(base_descriptor, extra_descriptor)?;
                        },
                        None => {
                            base_resolution.dependencies.insert(dep_ident, extra_descriptor);
                        },
                    }
                }
            }
        }
    }

    // Second pass: add descriptor-to-locator mappings for transitive
    // dependencies declared in each resolution. The tree resolver needs
    // these to look up dependency descriptors (e.g. no-deps@npm:^1.0.0)
    // that appear in a package's resolution.dependencies.
    for resolution in normalized_resolutions.values() {
        for (dep_ident, dep_descriptor) in &resolution.dependencies {
            if let Some(dep_locator) = ident_to_locator.get(dep_ident) {
                descriptor_to_locator
                    .entry(dep_descriptor.clone())
                    .or_insert_with(|| dep_locator.clone());
            }
        }
    }

    Ok(IslandResolutionResult {
        island_id: island_id.to_string(),
        descriptor_to_locator,
        normalized_resolutions,
    })
}

fn handle_pubgrub_error(
    island_id: &str,
    error: pubgrub::PubGrubError<IslandDependencyProvider<'_>>,
) -> Error {
    let message = match error {
        pubgrub::PubGrubError::NoSolution(mut derivation_tree) => {
            derivation_tree.collapse_no_versions();
            let report = pubgrub::DefaultStringReporter::report(&derivation_tree);

            let preview: String = report
                .lines()
                .take(30)
                .collect::<Vec<_>>()
                .join("\n");

            format!("No solution found.\n\n{}", preview)
        }

        pubgrub::PubGrubError::ErrorChoosingVersion {
            source: e, ..
        }
        | pubgrub::PubGrubError::ErrorRetrievingDependencies {
            source: e, ..
        } => {
            format!("{}", e)
        }

        pubgrub::PubGrubError::ErrorInShouldCancel(e) => {
            format!("Cancelled: {}", e)
        }
    };

    Error::IslandResolutionFailed {
        island_id: island_id.to_string(),
        message,
    }
}
