use std::collections::{BTreeMap, BTreeSet};
use itertools::Itertools;
use zpm_primitives::{Ident, IdentGlob, Locator};
use zpm_utils::{IoResultExt, Path, ToHumanString};

use crate::{
    build,
    error::Error,
    fetchers::PackageData,
    install::Install,
    linker::{self, LinkResult},
    project::Project,
    tree_resolver::ResolutionTree,
};

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

pub async fn link_project_pnpm<'a>(project: &'a Project, install: &'a Install) -> Result<LinkResult, Error> {
    let tree
        = &install.install_state.resolution_tree;

    let nm_path = project.project_cwd
        .with_join_str("node_modules");
    let store_path = project.project_cwd
        .with_join_str(&project.config.settings.pnpm_store_folder.value);

    // Remove existing node_modules
    linker::helpers::fs_remove_nm(nm_path)?;

    let mut packages_by_location
        = BTreeMap::new();
    let mut locations_by_package
        = BTreeMap::new();

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

                linker::helpers::fs_extract_archive(
                    &package_store_path,
                    physical_package_data,
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

        if let Some(build_commands) = package_build_info.build_commands {
            package_build_entries.insert(
                locator.clone(),
                all_build_entries.len(),
            );

            all_build_entries.push(build::BuildRequest {
                cwd: package_location_rel,
                locator: locator.clone(),
                commands: build_commands,
                allowed_to_fail: install.install_state.resolution_tree.optional_builds.contains(locator),
                force_rebuild: false, // TODO: track this properly for pnpm
            });
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
    // This is at node_modules/.store/node_modules/<package-name>
    let store_nm_path
        = store_path.with_join_str("node_modules");

    for (ident, locator) in &hoisted_packages {
        let package_location = locations_by_package
            .get(*locator)
            .expect("Failed to find package location for hoisted package");

        let package_abs_path = project.project_cwd
            .with_join(package_location);

        let link_abs_path = store_nm_path
            .with_join_str(ident.as_str());

        let link_abs_dirname = link_abs_path
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

    // Create symlinks for publicly hoisted packages in root node_modules
    let root_nm_path
        = project.project_cwd.with_join_str("node_modules");

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

        let package_location = locations_by_package
            .get(*locator)
            .expect("Failed to find package location for publicly hoisted package");

        let package_abs_path = project.project_cwd
            .with_join(package_location);

        let link_abs_path = root_nm_path
            .with_join_str(ident.as_str());

        let link_abs_dirname = link_abs_path
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

    // Second pass: create symlinks in node_modules directories
    for (locator, resolution) in &tree.locator_resolutions {
        let workspace_path
            = project.try_workspace_by_locator(locator)?;

        let package_base_path = match workspace_path {
            Some(workspace_path) => workspace_path.path.clone(),
            None => store_path.with_join_str(&locator.slug()),
        };

        let physical_package_data = install.package_data.get(&locator.physical_locator())
            .unwrap_or_else(|| panic!("Failed to find physical package data for {}", locator.physical_locator().to_print_string()));

        let is_local
            = matches!(physical_package_data, PackageData::Local {..});

        for (dep_name, descriptor) in &resolution.dependencies {
            let dep_locator = tree.descriptor_to_locator
                .get(descriptor)
                .expect("Failed to find dependency resolution");

            if !is_local && !locator.reference.is_workspace_reference() {
                if let Some(hoisted_locator) = hoisted_packages.get(dep_name) {
                    // If the exact same version is hoisted, skip creating the symlink
                    // The package will resolve it through the store's shared node_modules
                    if *hoisted_locator == dep_locator {
                        continue;
                    }
                }
            }

            // node_modules/.store/@types-no-deps-npm-1.0.0-xyz/package
            let dep_rel_location = locations_by_package
                .get(dep_locator)
                .expect("Failed to find dependency location; it should have been registered a little earlier");

            // /path/to/project/node_modules/.store/@types-no-deps-npm-1.0.0-xyz/package
            let dep_abs_path = project.project_cwd
                .with_join(dep_rel_location);

            // /path/to/project/node_modules/@types/no-deps
            let link_abs_path = package_base_path
                .with_join(&dep_name.nm_subdir());

            // /path/to/project/node_modules/@types
            let link_abs_dirname = link_abs_path
                .dirname()
                .expect("Failed to get directory name");

            // ../.store/@types-no-deps-npm-1.0.0-xyz/package
            let symlink_target = dep_abs_path
                .relative_to(&link_abs_dirname);

            link_abs_path
                .fs_rm_file()
                .ok_missing()?
                .unwrap_or(&link_abs_path)
                .fs_create_parent()?
                .fs_symlink(&symlink_target)?;
        }
    }

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
