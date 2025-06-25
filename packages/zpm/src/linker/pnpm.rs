use std::collections::BTreeMap;
use zpm_utils::{OkMissing, ToHumanString};

use crate::{build::{self, BuildRequests}, error::Error, fetchers::PackageData, install::Install, linker, project::Project};

pub async fn link_project_pnpm<'a>(project: &'a mut Project, install: &'a mut Install) -> Result<BuildRequests, Error> {
    let tree = &install.install_state.resolution_tree;
    let nm_path = project.project_cwd.with_join_str("node_modules");
    let store_path = project.project_cwd.with_join_str(&project.config.project.pnpm_store_folder.value);

    // Remove existing node_modules
    linker::helpers::fs_remove_nm(nm_path)?;

    let mut all_build_entries = Vec::new();
    let mut package_build_entries = BTreeMap::new();

    // Get dependencies meta from package.json
    let dependencies_meta
        = linker::helpers::TopLevelConfiguration::from_project(project);

    // First pass: copy all packages to store
    for (locator, resolution) in &tree.locator_resolutions {
        let physical_package_data = install.package_data.get(&locator.physical_locator())
            .unwrap_or_else(|| panic!("Failed to find physical package data for {}", locator.physical_locator().to_print_string()));

        let package_location_abs = match &physical_package_data {
            PackageData::Local {..} => {
                physical_package_data.package_directory().clone()
            },

            _ => {
                let package_store_path = store_path
                    .with_join_str(&locator.slug())
                    .with_join_str("package");

                linker::helpers::fs_extract_archive(
                    &package_store_path,
                    physical_package_data,
                )?;

                package_store_path
            },
        };

        let package_location_rel = package_location_abs
            .relative_to(&project.project_cwd);

        install.install_state.packages_by_location.insert(package_location_rel.clone(), locator.clone());
        install.install_state.locations_by_package.insert(locator.clone(), package_location_rel.clone());

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
                tree_hash: install.install_state.locator_tree_hash(locator),
                allowed_to_fail: install.install_state.resolution_tree.optional_builds.contains(locator),
                force_rebuild: false, // TODO: track this properly for pnpm
            });
        }
    }

    // Second pass: create symlinks in node_modules directories
    for (locator, resolution) in &tree.locator_resolutions {
        // <empty path, if we assume the root workspace>
        let package_location
            = install.install_state.locations_by_package.get(locator)
                .expect("Failed to find package location; it should have been registered a little earlier");

        // /path/to/project
        let package_abs_path = project.project_cwd
            .with_join(package_location);

        let physical_package_data = install.package_data.get(&locator.physical_locator())
            .unwrap_or_else(|| panic!("Failed to find physical package data for {}", locator.physical_locator().to_print_string()));

        // /path/to/project/node_modules
        // /path/to/project/node_modules/.store/@types-no-deps-npm-1.0.0-xyz/node_modules
        let package_abs_nm_path = if matches!(physical_package_data, PackageData::Local {..}) {
            package_abs_path.with_join_str("node_modules")
        } else {
            package_abs_path.dirname().unwrap().with_join_str("node_modules")
        };

        for (dep_name, descriptor) in &resolution.dependencies {
            let dep_locator = tree.descriptor_to_locator.get(descriptor)
                .expect("Failed to find dependency resolution");

            // node_modules/.store/@types-no-deps-npm-1.0.0-xyz/package
            let dep_rel_location
                = install.install_state.locations_by_package.get(dep_locator)
                    .expect("Failed to find dependency location; it should have been registered a little earlier");

            // /path/to/project/node_modules/.store/@types-no-deps-npm-1.0.0-xyz/package
            let dep_abs_path = project.project_cwd
                .with_join(dep_rel_location);

            // /path/to/project/node_modules/@types/no-deps
            let link_abs_path = package_abs_nm_path
                .with_join_str(dep_name.as_str());

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

        if !resolution.dependencies.contains_key(&locator.ident) {
            // We don't install self-dependencies in pnpm mode because it could lead
            // to filesystem loops for tools that traverse node_modules.
            if !matches!(physical_package_data, PackageData::Local {..}) {
                let self_link_abs_path = package_abs_nm_path
                    .with_join_str(locator.ident.as_str());

                let self_link_abs_dirname = self_link_abs_path
                    .dirname()
                    .expect("Failed to get directory name");

                let symlink_target = package_abs_path
                    .relative_to(&self_link_abs_dirname);

                self_link_abs_path
                    .fs_rm_file()
                    .ok_missing()?
                    .unwrap_or(&self_link_abs_path)
                    .fs_create_parent()?
                    .fs_symlink(&symlink_target)?;
            }
        }
    }

    let package_build_dependencies = linker::helpers::populate_build_entry_dependencies(
        &package_build_entries,
        &tree.locator_resolutions,
        &tree.descriptor_to_locator,
    );

    Ok(build::BuildRequests {
        entries: all_build_entries,
        dependencies: package_build_dependencies?,
    })
}
