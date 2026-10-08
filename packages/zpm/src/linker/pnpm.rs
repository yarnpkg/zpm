use std::collections::{BTreeMap, BTreeSet};
use itertools::Itertools;
use zpm_primitives::{Ident, IdentGlob, Locator};
use zpm_utils::{IoResultExt, Path, ToHumanString};

use crate::{
    build,
    error::Error,
    fetchers::PackageData,
    install::Install,
    linker::{self, LinkResult, package_map::{PnpmPackageMapBuilder, persist_package_map}},
    project::Project,
    tree_resolver::ResolutionTree,
};

/// Check if an ident matches any of the given glob patterns.
fn matches_patterns(ident: &Ident, patterns: &[IdentGlob]) -> bool {
    patterns.iter().any(|pattern| pattern.check(ident))
}

/// Collect all packages that should be hoisted based on patterns.
/// When several versions match, the one closest to the workspaces wins
/// (then the lowest locator), like pnpm which hoists in depth order: the
/// version the workspaces use directly is the one their transitive
/// dependencies most likely expect to find.
fn collect_hoistable_packages<'a>(tree: &'a ResolutionTree, patterns: &[IdentGlob], locations_by_package: &BTreeMap<Locator, Path>) -> BTreeMap<Ident, &'a Locator> {
    let depths
        = locator_depths(tree);

    let mut hoistable: BTreeMap<Ident, (usize, &'a Locator)>
        = BTreeMap::new();

    for locator in tree.locator_resolutions.keys() {
        // Skip local/workspace packages for hoisting
        if locator.reference.is_workspace_reference() {
            continue;
        }

        if !matches_patterns(&locator.ident, patterns) {
            continue;
        }

        // Only hoist packages that have a location in the store
        if !locations_by_package.contains_key(locator) {
            continue;
        }

        let depth
            = depths.get(locator).copied().unwrap_or(usize::MAX);

        let is_better = match hoistable.get(&locator.ident) {
            Some((existing_depth, existing_locator)) => (depth, locator) < (*existing_depth, *existing_locator),
            None => true,
        };

        if is_better {
            hoistable.insert(locator.ident.clone(), (depth, locator));
        }
    }

    hoistable.into_iter()
        .map(|(ident, (_, locator))| (ident, locator))
        .collect()
}

/// The distance of each package from the closest workspace (breadth-first).
fn locator_depths(tree: &ResolutionTree) -> BTreeMap<&Locator, usize> {
    let mut depths
        = BTreeMap::new();

    let mut queue: std::collections::VecDeque<(&Locator, usize)>
        = tree.locator_resolutions.keys()
            .filter(|locator| locator.reference.is_workspace_reference())
            .map(|locator| (locator, 0))
            .collect();

    while let Some((locator, depth)) = queue.pop_front() {
        if depths.contains_key(locator) {
            continue;
        }

        depths.insert(locator, depth);

        let Some(resolution) = tree.locator_resolutions.get(locator) else {
            continue;
        };

        for descriptor in resolution.dependencies.values() {
            if let Some((dependency_locator, _)) = tree.descriptor_to_locator.get(descriptor).and_then(|locator| tree.locator_resolutions.get_key_value(locator)) {
                if !depths.contains_key(dependency_locator) {
                    queue.push_back((dependency_locator, depth + 1));
                }
            }
        }
    }

    depths
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

    // Remove existing node_modules
    linker::helpers::fs_remove_nm(nm_path)?;

    let mut packages_by_location
        = BTreeMap::new();
    let mut locations_by_package
        = BTreeMap::new();
    let mut package_map_builder
        = PnpmPackageMapBuilder::new(project);

    let mut all_build_entries
        = Vec::new();
    let mut package_build_entries
        = BTreeMap::new();

    // Get dependencies meta from package.json
    let dependencies_meta
        = linker::helpers::TopLevelConfiguration::from_project(project);

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

                linker::helpers::fs_extract_archive_with_cas(
                    &package_store_path,
                    physical_package_data,
                    &cas_index_root,
                )?;

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

        link_abs_path
            .fs_rm_file()
            .ok_missing()?
            .unwrap_or(&link_abs_path)
            .fs_create_parent()?
            .fs_symlink(&symlink_target)?;
    }

    // Public hoisting targets the root node_modules, so only the root
    // workspace's own dependencies take precedence over it (as with pnpm);
    // other workspaces declaring the package get their own links, but
    // packages resolving from the root (for example a workspace importing
    // it through another workspace) still need the hoisted one
    let root_dependency_idents: BTreeSet<Ident>
        = tree.locator_resolutions.get(&project.root_workspace().locator())
            .map(|resolution| resolution.dependencies.keys().cloned().collect())
            .unwrap_or_default();

    for (ident, locator) in &public_hoisted_packages {
        if root_dependency_idents.contains(ident) {
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

        let link_abs_dirname
            = link_abs_path
                .dirname()
                .expect("Failed to get directory name");

        let symlink_target
            = package_abs_path
                .relative_to(&link_abs_dirname);

        link_abs_path
            .fs_rm_file()
            .ok_missing()?
            .unwrap_or(&link_abs_path)
            .fs_create_parent()?
            .fs_symlink(&symlink_target)?;
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

            link_abs_path
                .fs_rm_file()
                .ok_missing()?
                .unwrap_or(&link_abs_path)
                .fs_create_parent()?
                .fs_symlink(&symlink_target)?;
        }

        if !has_explicit_self_dependency && !locator.reference.is_workspace_reference() {
            package_map_builder.register_dependency(locator, &locator.ident, locator)?;
        }
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
