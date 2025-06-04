use std::collections::BTreeMap;

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
            .unwrap_or_else(|| panic!("Failed to find physical package data for {}", locator.physical_locator()));

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
        let physical_package_data = install.package_data.get(&locator.physical_locator())
            .unwrap_or_else(|| panic!("Failed to find physical package data for {}", locator.physical_locator()));

        // Determine where this package lives
        let package_location = if matches!(physical_package_data, PackageData::Local {..}) {
            // Workspace packages live in their original location
            physical_package_data.package_directory()
        } else {
            // Regular packages live in the store
            &store_path
                .with_join_str(&locator.slug())
                .with_join_str("package")
        };

        // Create node_modules directory for this package
        let package_nm_path = package_location.with_join_str("node_modules");
        package_nm_path.fs_create_dir_all()?;

        // Create symlinks for all dependencies
        for (dep_name, descriptor) in &resolution.dependencies {
            let dep_locator = tree.descriptor_to_locator.get(descriptor)
                .expect("Failed to find dependency resolution");
            
            let dep_physical_package_data = install.package_data.get(&dep_locator.physical_locator())
                .unwrap_or_else(|| panic!("Failed to find physical package data for {}", dep_locator.physical_locator()));

            let dep_target_path = if matches!(dep_physical_package_data, PackageData::Local {..}) {
                // Workspace dependency - link to workspace location
                dep_physical_package_data.package_directory()
            } else {
                // Regular dependency - link to store location
                &store_path
                    .with_join_str(&dep_locator.slug())
                    .with_join_str("package")
            };

            let link_path = package_nm_path.with_join_str(&dep_name.name());
            
            // Create symlink (remove existing one if present)
            if link_path.fs_exists() {
                link_path.fs_rm()?;
            }
            
            // Create symlink - use relative path for portability
            let relative_target = dep_target_path.relative_to(&package_nm_path);
            link_path.fs_symlink(&relative_target)?;
        }
    }

    // Create root node_modules with workspace dependencies
    let root_nm_path = project.project_cwd.with_join_str("node_modules");
    root_nm_path.fs_create_dir_all()?;

    // Link workspace packages in root node_modules
    for workspace in &project.workspaces {
        if workspace.path != project.project_cwd {
            let link_path = root_nm_path.with_join_str(&workspace.name.name());
            if link_path.fs_exists() {
                link_path.fs_rm()?;
            }
            let relative_target = workspace.path.relative_to(&root_nm_path);
            link_path.fs_symlink(&relative_target)?;
        }
    }

    // Create symlinks in root node_modules for all root workspace dependencies
    let root_workspace = &project.workspaces[0]; // First workspace is always the root
    let root_locator = root_workspace.locator();
    
    if let Some(root_resolution) = tree.locator_resolutions.get(&root_locator) {
        for (dep_name, descriptor) in &root_resolution.dependencies {
            let dep_locator = tree.descriptor_to_locator.get(descriptor)
                .expect("Failed to find dependency resolution");
            
            let dep_physical_package_data = install.package_data.get(&dep_locator.physical_locator())
                .unwrap_or_else(|| panic!("Failed to find physical package data for {}", dep_locator.physical_locator()));

            let dep_target_path = if matches!(dep_physical_package_data, PackageData::Local {..}) {
                // Workspace dependency - link to workspace location
                dep_physical_package_data.package_directory()
            } else {
                // Regular dependency - link to store location
                &store_path
                    .with_join_str(&dep_locator.slug())
                    .with_join_str("package")
            };

            let link_path = root_nm_path.with_join_str(&dep_name.name());
            
            // Create symlink (remove existing one if present)
            if link_path.fs_exists() {
                link_path.fs_rm()?;
            }
            
            // Create symlink - use relative path for portability
            let relative_target = dep_target_path.relative_to(&root_nm_path);
            link_path.fs_symlink(&relative_target)?;
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
