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
    pub input_hash: Option<zpm_utils::Hash64>,
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
    // Phase 2: Build locked_versions map (preferred locators from lockfile,
    // or from the uv.lock the island was imported from)
    let mut locked_versions = build_locked_versions(island, lockfile);

    // The seed fills the gaps of the lockfile rather than only replacing a
    // missing one: packages added since the last install (say, new extras
    // requested from the workspace) keep the version uv had locked instead
    // of jumping to the latest release
    for (ident, locator) in seed_locked_versions(ctx, island) {
        locked_versions.entry(ident).or_insert(locator);
    }

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

    // Phase 1: lockfile fast path. The island is reused as-is when none of
    // its inputs changed since it was locked.
    let input_hash
        = island_input_hash(&island_context(ctx, island), island, &workspace_deps);

    if !ctx.refresh_lockfile && lockfile.island_hashes.get(&island.id) == Some(&input_hash) {
        if let Some(locked_island) = lockfile.islands.get(&island.id) {
            let is_complete
                = locked_island.values().all(|locator| lockfile.entries.contains_key(locator));

            if is_complete {
                let mut result
                    = island_result_from_lockfile(&island.id, locked_island, lockfile)?;

                // Workspaces aren't stored in the lockfile; the island's own
                // workspaces and the ones they depend on are added back
                let mut workspace_queue
                    = workspace_deps.keys().cloned().collect::<Vec<_>>();

                let mut seen_workspaces
                    = BTreeSet::new();

                while let Some(ident) = workspace_queue.pop() {
                    if !seen_workspaces.insert(ident.clone()) {
                        continue;
                    }

                    let deps = match workspace_deps.get(&ident) {
                        Some(deps) => deps.clone(),
                        None => match project.workspace_by_ident(&ident) {
                            Ok(workspace) => workspace.manifest.remote.dependencies.clone(),
                            Err(_) => continue,
                        },
                    };

                    let locator = Locator::new(ident.clone(), WorkspaceIdentReference {ident: ident.clone()}.into());
                    let descriptor = Descriptor::new(ident.clone(), zpm_primitives::WorkspaceMagicRange {magic: zpm_semver::RangeKind::Caret}.into());

                    for dependency in deps.values() {
                        if dependency.range.is_workspace() || project.try_workspace_by_descriptor(dependency).ok().flatten().is_some() {
                            result.descriptor_to_locator.insert(dependency.clone(), Locator::new(dependency.ident.clone(), WorkspaceIdentReference {ident: dependency.ident.clone()}.into()));
                            workspace_queue.push(dependency.ident.clone());
                        }
                    }

                    let mut resolution = Resolution::new_empty(locator.clone(), zpm_semver::Version::default());
                    resolution.dependencies = deps;

                    result.descriptor_to_locator.insert(descriptor, locator.clone());
                    result.normalized_resolutions.insert(locator, resolution);
                }

                result.input_hash = Some(input_hash);
                return Ok(result);
            }
        }
    }

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

    let island_ctx
        = island_context(ctx, island);

    let ctx = &island_ctx;

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
    // Workspaces from other islands appear in the solution with their
    // regular dependencies only
    let mut all_workspace_deps
        = workspace_deps.clone();

    for workspace in &project.workspaces {
        all_workspace_deps.entry(workspace.name.clone())
            .or_insert_with(|| workspace.manifest.remote.dependencies.clone());
    }

    let mut result
        = convert_solution(&island.id, solution, &resolution_cache, &extra_resolution_cache, &all_workspace_deps)?;

    resolve_variants(ctx, &mut result).await?;

    result.input_hash = Some(input_hash);

    Ok(result)
}

/// Hash of everything an island's resolution depends on, besides the
/// registry content itself.
fn island_input_hash(ctx: &InstallContext<'_>, island: &ResolvedIsland, workspace_deps: &BTreeMap<Ident, BTreeMap<Ident, Descriptor>>) -> zpm_utils::Hash64 {
    use zpm_utils::ToFileString;

    let mut parts
        = vec!["island-v1".to_string(), island.id.clone()];

    for (ident, deps) in workspace_deps {
        parts.push(format!("ws:{}", ident.to_file_string()));

        for descriptor in deps.values() {
            parts.push(descriptor.to_file_string());
        }
    }

    for (selector, range) in ctx.dependency_overrides.iter() {
        parts.push(format!("override:{}={}", selector.to_file_string(), range.as_ref().map(|range| range.to_file_string()).unwrap_or_default()));
    }

    for constraint in &ctx.pypi_constraints {
        parts.push(format!("constraint:{}", constraint));
    }

    if let Some(project) = ctx.project {
        let targets
            = crate::python_env::PythonTargets::from_config(&project.config, ctx.python_version.as_deref());

        parts.push(targets.fingerprint());
        parts.push(format!("registry:{}", project.config.settings.pypi_registry_server.value));
        parts.push(format!("age-gate:{:?}", project.config.settings.pypi_minimal_age_gate.value));

        for rule in &project.config.settings.package_rules {
            parts.push(format!("rule:{:?}:{:?}", rule.package_filter.value.as_ref().map(|filter| filter.to_file_string()), rule.pypi_registry_server.value));
        }
    }

    zpm_utils::Hash64::from_data(parts.join("\n").as_bytes())
}

/// The install context used to resolve an island:
///
/// - `pythonVersion` can be overridden per island;
/// - the `resolutions` of the island's workspaces apply to the island (on
///   top of the root workspace's), which lets each Python project keep its
///   own overrides (uv's `override-dependencies`);
/// - `pypiConstraints` restrict the candidate versions of the packages
///   they mention (uv's `constraint-dependencies`).
fn island_context<'a>(ctx: &InstallContext<'a>, island: &ResolvedIsland) -> InstallContext<'a> {
    let mut island_ctx
        = ctx.clone();

    let Some(project) = ctx.project else {
        return island_ctx;
    };

    let definition
        = project.config.settings.unstable_islands.get(&island.id);

    island_ctx.python_version = definition
        .and_then(|definition| definition.python_version.value.clone());

    island_ctx.pypi_constraints = definition
        .map(|definition| definition.pypi_constraints.iter().map(|constraint| constraint.value.clone()).collect())
        .unwrap_or_default();

    let root_ident
        = project.root_workspace().name.clone();

    let island_overrides
        = project.workspaces.iter()
            .filter(|workspace| workspace.name != root_ident && island.workspace_idents.contains(&workspace.name))
            .flat_map(|workspace| workspace.manifest.resolutions.iter().map(|(selector, range)| (selector.clone(), range.clone())))
            .collect::<Vec<_>>();

    if !island_overrides.is_empty() {
        // Island rules come first so they take precedence over the root ones
        let entries
            = island_overrides.into_iter()
                .chain(ctx.dependency_overrides.iter().map(|(selector, range)| (selector.clone(), range.clone())));

        island_ctx.dependency_overrides
            = std::sync::Arc::new(crate::manifest::resolutions::ResolutionsField::from_entries(entries));
    }

    island_ctx
}

/// Platform variants (packages with per-platform artifacts) aren't part
/// of the solver's graph; they're resolved once the solution is known.
async fn resolve_variants(ctx: &InstallContext<'_>, result: &mut IslandResolutionResult) -> Result<(), Error> {
    let variants
        = result.normalized_resolutions.values()
            .flat_map(|resolution| resolution.variants.iter().map(move |variant| (variant.clone(), resolution.dependencies.clone())))
            .collect::<BTreeMap<_, _>>();

    let futures = variants.keys().map(|descriptor| {
        crate::resolvers::resolve_descriptor(ctx.clone(), descriptor.clone(), vec![])
    });

    let resolved
        = futures::future::try_join_all(futures).await?;

    // The tree resolver substitutes the variant to its parent, so the
    // variant must carry the dependencies the solver picked for the parent
    for ((descriptor, dependencies), resolution) in variants.into_iter().zip(resolved) {
        let mut resolution
            = resolution.resolution;

        resolution.dependencies = dependencies;

        let locator
            = resolution.locator.clone();

        result.descriptor_to_locator.insert(descriptor, locator.clone());

        // Variants for several platforms can resolve to the same artifact
        // (a macOS universal2 wheel serves both arm64 and x64); they share a
        // locator, which must then accept every one of these platforms
        if let Some(existing) = result.normalized_resolutions.get_mut(&locator) {
            if !existing.variants.is_empty() || existing.requirements == resolution.requirements {
                continue;
            }

            existing.requirements.extend(&resolution.requirements);
            continue;
        }

        result.normalized_resolutions.insert(locator, resolution);
    }

    Ok(())
}

/// Build an IslandResolutionResult from cached lockfile data.
fn island_result_from_lockfile(
    island_id: &str,
    locked_island: &BTreeMap<Descriptor, Locator>,
    lockfile: &Lockfile,
) -> Result<IslandResolutionResult, Error> {
    let mut normalized_resolutions = BTreeMap::new();

    for locator in locked_island.values() {
        if let Some(entry) = lockfile.entries.get(locator) {
            let mut resolution
                = entry.resolution.clone();

            // Only keep the variants this island resolved
            resolution.variants.retain(|variant| locked_island.contains_key(variant));

            normalized_resolutions.insert(locator.clone(), resolution);
        }
    }

    Ok(IslandResolutionResult {
        island_id: island_id.to_string(),
        input_hash: None,
        descriptor_to_locator: locked_island.clone(),
        normalized_resolutions,
    })
}

/// Reads the versions pinned by the island's seed uv.lock (`pypiSeedLockfile`)
/// as preferred versions. Only registry packages are taken into account.
fn seed_locked_versions(ctx: &InstallContext<'_>, island: &ResolvedIsland) -> BTreeMap<Ident, Locator> {
    let mut locked
        = BTreeMap::new();

    let Some(project) = ctx.project else {
        return locked;
    };

    let Some(seed) = project.config.settings.unstable_islands.get(&island.id).and_then(|definition| definition.pypi_seed_lockfile.value.clone()) else {
        return locked;
    };

    let Ok(text) = project.project_cwd.with_join_str(&seed).fs_read_text() else {
        return locked;
    };

    let Ok(document) = text.parse::<toml_edit::DocumentMut>() else {
        return locked;
    };

    let Some(packages) = document.get("package").and_then(|packages| packages.as_array_of_tables()) else {
        return locked;
    };

    for package in packages.iter() {
        let is_registry
            = package.get("source").and_then(|source| source.as_inline_table()).map_or(false, |source| source.contains_key("registry"));

        if !is_registry {
            continue;
        }

        let (Some(name), Some(version)) = (package.get("name").and_then(|v| v.as_str()), package.get("version").and_then(|v| v.as_str())) else {
            continue;
        };

        let Ok(ident) = Ident::from_file_string(&zpm_primitives::canonicalize_pypi_name(name)) else {
            continue;
        };

        let Ok(version) = zpm_primitives::PypiVersion::from_file_string(version) else {
            continue;
        };

        // A package can appear multiple times in forked uv locks; the first
        // (highest Python) entry wins
        locked.entry(ident.clone()).or_insert_with(|| Locator::new(ident, zpm_primitives::PypiShorthandReference {version, url: None}.into()));
    }

    locked
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
        input_hash: None,
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
