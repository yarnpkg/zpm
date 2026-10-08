use std::collections::{BTreeMap, BTreeSet};
use itertools::Itertools;
use rayon::prelude::*;
use zpm_primitives::{Ident, IdentGlob, Locator};
use zpm_utils::{Hash64, IoResultExt, Path, ToHumanString};

use crate::{
    build,
    error::Error,
    fetchers::PackageData,
    install::Install,
    linker::{self, LinkResult, package_map::{self, PackageMap, PnpmPackageMapBuilder, persist_package_map}},
    project::Project,
    tree_resolver::ResolutionTree,
};

/// Creates (or repairs) a symlink at `link_abs_path` pointing at
/// `symlink_target`. Links that already point at the right place are
/// left untouched, which keeps warm installs from rewriting tens of
/// thousands of symlinks.
fn ensure_symlink(link_abs_path: &Path, symlink_target: &Path) -> Result<(), Error> {
    if link_abs_path.fs_read_link().ok().as_ref() == Some(symlink_target) {
        return Ok(());
    }

    if link_abs_path.fs_is_symlink() || link_abs_path.fs_is_file() {
        link_abs_path.fs_rm_file()?;
    } else if link_abs_path.fs_exists() {
        link_abs_path.fs_rm()?;
    }

    link_abs_path
        .fs_create_parent()?
        .fs_symlink(symlink_target)?;

    Ok(())
}

/// Removes the entries of `dir_path` that aren't listed in `keep`. Used to
/// prune store entries and top-level links left by a previous install
/// without wiping (and re-extracting) everything else.
fn prune_dir_entries(dir_path: &Path, keep: &BTreeSet<String>) -> Result<(), Error> {
    let Some(entries) = dir_path.fs_read_dir().ok_missing()? else {
        return Ok(());
    };

    for entry in entries.flatten() {
        let name
            = entry.file_name().to_string_lossy().to_string();

        if keep.contains(&name) {
            continue;
        }

        let entry_path
            = dir_path.with_join_str(&name);

        if entry_path.fs_is_symlink() || entry_path.fs_is_file() {
            entry_path.fs_rm_file()?;
        } else {
            entry_path.fs_rm()?;
        }
    }

    Ok(())
}

/// Check if an ident matches any of the given glob patterns.
fn matches_patterns(ident: &Ident, patterns: &[IdentGlob]) -> bool {
    patterns.iter().any(|pattern| pattern.check(ident))
}

/// Collect all packages that should be hoisted based on patterns.
/// Returns a map from ident to the locator that should be hoisted (picks the first one found for conflicts).
fn collect_hoistable_packages<'a>(tree: &'a ResolutionTree, patterns: &[IdentGlob], locations_by_package: &BTreeMap<Locator, Path>) -> BTreeMap<Ident, &'a Locator> {
    let mut hoistable
        = BTreeMap::new();

    for locator in tree.locator_resolutions.keys() {
        // Skip local/workspace packages for hoisting
        if locator.reference.is_workspace_reference() {
            continue;
        }

        // Check if this package matches any hoist pattern
        if matches_patterns(&locator.ident, patterns) {
            // Only hoist if we haven't seen this ident yet (first wins for conflicts)
            if !hoistable.contains_key(&locator.ident) {
                // Only hoist packages that have a location in the store
                if locations_by_package.contains_key(locator) {
                    hoistable.insert(locator.ident.clone(), locator);
                }
            }
        }
    }

    hoistable
}

/// Removes links in a `node_modules` folder that the new layout doesn't
/// create anymore (removed dependencies, unhoisted packages). Dot-entries
/// such as `.pnpm` or `.bin` are left alone; scoped folders are pruned
/// one level deeper.
fn prune_node_modules(nm_path: &Path, expected: &BTreeSet<Ident>) -> Result<(), Error> {
    let mut top_level
        = BTreeSet::new();
    let mut by_scope: BTreeMap<String, BTreeSet<String>>
        = BTreeMap::new();

    for ident in expected {
        match ident.scope() {
            Some(scope) => {
                top_level.insert(scope.to_string());
                by_scope.entry(scope.to_string()).or_default().insert(ident.name().to_string());
            },

            None => {
                top_level.insert(ident.as_str().to_string());
            },
        }
    }

    let Some(entries) = nm_path.fs_read_dir().ok_missing()? else {
        return Ok(());
    };

    for entry in entries.flatten() {
        let name
            = entry.file_name().to_string_lossy().to_string();

        if name.starts_with('.') {
            continue;
        }

        let entry_path
            = nm_path.with_join_str(&name);

        if let Some(scoped_names) = by_scope.get(&name) {
            prune_dir_entries(&entry_path, scoped_names)?;
        } else if !top_level.contains(&name) {
            if entry_path.fs_is_symlink() || entry_path.fs_is_file() {
                entry_path.fs_rm_file()?;
            } else {
                entry_path.fs_rm()?;
            }
        }
    }

    Ok(())
}

pub async fn link_project_pnpm<'a>(project: &'a Project, install: &'a Install) -> Result<LinkResult, Error> {
    let tree
        = &install.install_state.resolution_tree;

    let nm_path = project.project_cwd
        .with_join_str("node_modules");
    let store_path = project.project_cwd
        .with_join_str(&project.config.settings.pnpm_store_folder.value);

    let cas_index_root = project.config.settings.global_folder.value
        .with_join_str("index");

    // The previous package map records which locator and archive checksum
    // each store folder was materialized from. Store folders are keyed by
    // locator slug, so a matching entry means the folder can be kept as-is.
    let previous_map: Option<PackageMap>
        = (!install.force)
            .then(|| package_map::load_package_map(&project.package_map_path(None)))
            .flatten();

    let mut packages_by_location
        = BTreeMap::new();
    let mut locations_by_package
        = BTreeMap::new();
    let mut package_map_builder
        = PnpmPackageMapBuilder::new(project, install);

    let mut all_build_entries
        = Vec::new();
    let mut package_build_entries
        = BTreeMap::new();

    // Get dependencies meta from package.json
    let dependencies_meta
        = linker::helpers::TopLevelConfiguration::from_project(project);

    let mut store_slugs
        = BTreeSet::new();
    let mut expected_nm_entries: BTreeMap<Path, BTreeSet<Ident>>
        = BTreeMap::new();
    let mut extractions
        = Vec::new();

    // First pass: copy all packages to store
    for (locator, resolution) in &tree.locator_resolutions {
        let physical_package_data = install.package_data
            .get(&locator.physical_locator())
            .unwrap_or_else(|| panic!("Failed to find physical package data for {}", locator.physical_locator().to_print_string()));

        let package_base_path = store_path
            .with_join_str(&locator.slug());

        let package_location_abs = match &physical_package_data {
            PackageData::Local {..} => {
                physical_package_data.package_directory().clone()
            },

            _ => {
                let package_store_path = package_base_path
                    .with_join(&locator.ident.nm_subdir());

                store_slugs.insert(locator.slug());

                let physical_locator
                    = locator.physical_locator();

                let checksum: Option<&Hash64> = install.lockfile.entries
                    .get(&physical_locator)
                    .and_then(|entry| entry.checksum.as_ref())
                    .or_else(|| install.install_state.cache_checksums.get(&physical_locator));

                let package_id
                    = package_map::get_package_id(&nm_path, &package_store_path.without_trailing_separators());

                let assume_up_to_date = package_store_path.with_join_str(".ready").fs_exists()
                    && previous_map.as_ref().is_some_and(|previous_map| previous_map.matches_package(&package_id, locator, checksum));

                if !assume_up_to_date {
                    extractions.push((package_store_path.clone(), physical_locator));
                }

                package_store_path
            },
        };

        let package_location_rel = package_location_abs
            .relative_to(&project.project_cwd);

        packages_by_location.insert(
            package_location_rel.clone(),
            locator.clone(),
        );

        locations_by_package.insert(
            locator.clone(),
            package_location_rel.clone(),
        );

        package_map_builder.register_package(locator, package_location_abs.clone());

        // We don't create node_modules directories and we don't build
        // local packages that are not fully contained within the project
        if matches!(physical_package_data, PackageData::Local {package_directory, ..} if !project.project_cwd.contains(package_directory)) {
            continue;
        }

        // Handle build requirements (similar to PnP logic)
        let package_build_info = linker::helpers::get_package_internal_info(
            project,
            install,
            &dependencies_meta,
            locator,
            resolution,
            physical_package_data,
        );

        if !package_build_info.build_step.is_noop() {
            // Virtualized locators share their build with the physical
            // counterpart — only the physical entry should drive a build.
            if !locator.reference.is_virtual_reference() {
                package_build_entries.insert(
                    locator.clone(),
                    all_build_entries.len(),
                );

                all_build_entries.push(build::BuildRequest {
                    cwd: package_location_rel,
                    locator: locator.clone(),
                    build_step: package_build_info.build_step,
                    allowed_to_fail: install.install_state.resolution_tree.optional_builds.contains(locator),
                    force_rebuild: false, // TODO: track this properly for pnpm
                    inline_builds: install.inline_builds,
                });
            }
        }
    }

    // Store entries whose locator is gone from the tree are leftovers from a
    // previous install; dot-entries (.ready, .modules.yaml, ...) are kept.
    let mut kept_store_entries
        = store_slugs;
    kept_store_entries.insert("node_modules".to_string());
    kept_store_entries.insert("lock.yaml".to_string());

    if let Some(store_entries) = store_path.fs_read_dir().ok_missing()? {
        for entry in store_entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                kept_store_entries.insert(name);
            }
        }
    }

    prune_dir_entries(&store_path, &kept_store_entries)?;

    tokio::task::block_in_place(|| {
        extractions.par_iter().try_for_each(|(package_store_path, physical_locator)| -> Result<(), Error> {
            let package_data = install.package_data
                .get(physical_locator)
                .unwrap_or_else(|| panic!("Expected package data for {}", physical_locator.to_print_string()));

            // A folder that doesn't match the previous map may hold stale
            // files from another archive; start from a clean slate.
            if package_store_path.fs_exists() {
                package_store_path.fs_rm()?;
            }

            linker::helpers::fs_extract_archive_with_cas(package_store_path, package_data, &cas_index_root)?;

            Ok(())
        })
    })?;

    let hoist_patterns
        = project.config.settings.pnpm_hoist_patterns
            .iter()
            .map(|s| s.value.clone())
            .collect_vec();

    let hoisted_packages
        = collect_hoistable_packages(tree, &hoist_patterns, &locations_by_package);

    let public_hoist_patterns
        = project.config.settings.pnpm_public_hoist_patterns
            .iter()
            .map(|s| s.value.clone())
            .collect_vec();

    let public_hoisted_packages
        = collect_hoistable_packages(tree, &public_hoist_patterns, &locations_by_package);

    // Create symlinks for hoisted packages in the store's shared node_modules
    // This is at node_modules/.pnpm/node_modules/<package-name>
    for (ident, locator) in &hoisted_packages {
        let package_location = locations_by_package
            .get(*locator)
            .expect("Failed to find package location for hoisted package");

        let package_abs_path
            = project.project_cwd
                .with_join(package_location);

        let link_abs_path
            = store_path
                .with_join(&ident.nm_subdir());

        let link_abs_dirname
            = link_abs_path
                .dirname()
                .expect("Failed to get directory name");

        let symlink_target = package_abs_path
            .relative_to(&link_abs_dirname);

        ensure_symlink(&link_abs_path, &symlink_target)?;
    }

    // Track which packages are direct dependencies of workspaces
    let mut direct_dependency_idents: BTreeSet<Ident> = BTreeSet::new();
    for (locator, resolution) in &tree.locator_resolutions {
        if locator.reference.is_workspace_reference() {
            for dep_ident in resolution.dependencies.keys() {
                direct_dependency_idents.insert(dep_ident.clone());
            }
        }
    }

    for (ident, locator) in &public_hoisted_packages {
        // Skip if this is already a direct dependency (will be linked separately)
        if direct_dependency_idents.contains(ident) {
            continue;
        }

        let package_location
            = locations_by_package
                .get(*locator)
                .expect("Failed to find package location for publicly hoisted package");

        let package_abs_path
            = project.project_cwd
                .with_join(package_location);

        let link_abs_path
            = project.project_cwd
                .with_join(&ident.nm_subdir());

        expected_nm_entries
            .entry(nm_path.clone())
            .or_default()
            .insert((*ident).clone());

        let link_abs_dirname
            = link_abs_path
                .dirname()
                .expect("Failed to get directory name");

        let symlink_target
            = package_abs_path
                .relative_to(&link_abs_dirname);

        ensure_symlink(&link_abs_path, &symlink_target)?;
    }

    // Second pass: create symlinks in node_modules directories
    for (locator, resolution) in &tree.locator_resolutions {
        let workspace
            = project.try_workspace_by_locator(locator)?;

        let physical_package_data = install.package_data.get(&locator.physical_locator())
            .unwrap_or_else(|| panic!("Failed to find physical package data for {}", locator.physical_locator().to_print_string()));

        let is_local
            = matches!(physical_package_data, PackageData::Local {..});

        let mut has_explicit_self_dependency
            = false;

        for (dep_name, descriptor) in &resolution.dependencies {
            if dep_name == &locator.ident {
                has_explicit_self_dependency = true;
            }

            let dep_locator
                = tree.descriptor_to_locator
                    .get(descriptor)
                    .expect("Failed to find dependency resolution");

            package_map_builder.register_dependency(locator, dep_name, dep_locator)?;

            if !is_local && !locator.reference.is_workspace_reference() {
                if let Some(hoisted_locator) = hoisted_packages.get(dep_name) {
                    // If the exact same version is hoisted, skip creating the symlink
                    // The package will resolve it through the store's shared node_modules
                    if *hoisted_locator == dep_locator {
                        continue;
                    }
                }
            }

            // node_modules/.pnpm/@types-no-deps-npm-1.0.0-xyz/node_modules/@types/no-deps
            let dep_rel_location
                = locations_by_package
                    .get(dep_locator)
                    .expect("Failed to find dependency location; it should have been registered a little earlier");

            // /path/to/project/node_modules/.pnpm/@types-no-deps-npm-1.0.0-xyz/node_modules/@types/no-deps
            let dep_abs_path
                = project.project_cwd
                    .with_join(dep_rel_location);

            // /path/to/project/node_modules/@types/no-deps
            if let Some(workspace) = workspace {
                expected_nm_entries
                    .entry(workspace.path.with_join_str("node_modules"))
                    .or_default()
                    .insert(dep_name.clone());
            }

            let link_abs_path = match workspace {
                Some(workspace) => workspace.path.with_join(&dep_name.nm_subdir()),
                None if dep_name == &locator.ident => store_path.with_join_str(&locator.slug()).with_join(&locator.ident.nm_subdir()).with_join(&dep_name.nm_subdir()),
                None => store_path.with_join_str(&locator.slug()).with_join(&dep_name.nm_subdir()),
            };

            // /path/to/project/node_modules/@types
            let link_abs_dirname
                = link_abs_path
                    .dirname()
                    .expect("Failed to get directory name");

            // ../.pnpm/@types-no-deps-npm-1.0.0-xyz/node_modules/@types/no-deps
            let symlink_target
                = dep_abs_path
                    .relative_to(&link_abs_dirname);

            ensure_symlink(&link_abs_path, &symlink_target)?;
        }

        if !has_explicit_self_dependency && !locator.reference.is_workspace_reference() {
            package_map_builder.register_dependency(locator, &locator.ident, locator)?;
        }
    }

    // Every workspace gets its node_modules pruned, including those whose
    // dependencies were all removed (absent from `expected_nm_entries`).
    for workspace in &project.workspaces {
        let workspace_nm_path
            = workspace.path.with_join_str("node_modules");

        prune_node_modules(&workspace_nm_path, expected_nm_entries.get(&workspace_nm_path).unwrap_or(&BTreeSet::new()))?;
    }

    persist_package_map(project, &package_map_builder.build()?)?;

    let package_build_dependencies = linker::helpers::populate_build_entry_dependencies(
        &package_build_entries,
        &tree.locator_resolutions,
        &tree.descriptor_to_locator,
    );

    let build_requests = build::BuildRequests {
        entries: all_build_entries,
        dependencies: package_build_dependencies?,
    };

    Ok(LinkResult {
        packages_by_location,
        build_requests,
    })
}
