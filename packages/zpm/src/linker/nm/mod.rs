use std::collections::BTreeMap;

use zpm_primitives::{Ident, Reference};
use zpm_sync::{SyncItem, SyncTemplate, SyncTree};
use zpm_utils::{FromFileString, Path, ToHumanString};

use crate::{
    build::BuildRequests, error::Error, fetchers::PackageData, install::Install, linker::{LinkResult, nm::hoist::{Hoister, WorkTree}}, project::Project
};

pub mod hoist;

const EXPECT_CHILDREN: &str = "All nodes should be expanded by the end of the hoisting process";

fn collect_binaries_from_dependencies(install: &Install, children: &BTreeMap<Ident, usize>, work_tree: &WorkTree) -> BTreeMap<String, (Ident, Path)> {
    let mut binaries
        = BTreeMap::new();

    for (ident, child_idx) in children {
        let child_node
            = &work_tree.nodes[*child_idx];

        let physical_locator
            = child_node.locator.physical_locator();

        if let Some(content_flags) = install.install_state.content_flags.get(&physical_locator) {
            for (bin_name, bin_path) in &content_flags.binaries {
                binaries.insert(bin_name.clone(), (ident.clone(), bin_path.clone()));
            }
        }
    }

    binaries
}

fn collect_workspace_binaries(install: &Install, workspace_node: &hoist::WorkNode) -> BTreeMap<String, (Ident, Path)> {
    let mut binaries
        = BTreeMap::new();

    let physical_locator
        = workspace_node.locator.physical_locator();

    if let Some(content_flags) = install.install_state.content_flags.get(&physical_locator) {
        for (bin_name, bin_path) in &content_flags.binaries {
            binaries.insert(bin_name.clone(), (workspace_node.locator.ident.clone(), bin_path.clone()));
        }
    }

    binaries
}

/// Registers bin symlinks in the sync tree for a given node_modules subfolder.
/// `node_rel_path` is the path within node_modules where dependencies are located
/// (e.g., "" for top-level, or "foo/node_modules" for nested).
fn register_bin_symlinks_at_path(workspace_nm_tree: &mut SyncTree, node_rel_path: &Path, binaries: &BTreeMap<String, (Ident, Path)>) -> Result<(), Error> {
    for (bin_name, (dep_ident, bin_path)) in binaries {
        // The symlink will be at <node_rel_path>/.bin/<bin_name>
        // It needs to point to ../<dep_name>/<bin_path>
        let bin_symlink_path = node_rel_path
            .with_join_str(".bin")
            .with_join_str(bin_name);

        let target_path = Path::new()
            .with_join_str("..")
            .with_join_str(&dep_ident.as_str())
            .with_join(bin_path);

        workspace_nm_tree.register_entry(bin_symlink_path, SyncItem::Symlink {
            target_path,
        })?;
    }

    Ok(())
}

fn register_workspace_bin_symlinks(workspace_nm_tree: &mut SyncTree, workspace_path: &Path, binaries: &BTreeMap<String, (Ident, Path)>) -> Result<(), Error> {
    for (bin_name, (_ident, bin_path)) in binaries {
        let target_abs_path
            = workspace_path
                .with_join(bin_path);

        if !target_abs_path.fs_exists() {
            continue;
        }

        let bin_symlink_path = Path::new()
            .with_join_str(".bin")
            .with_join_str(bin_name);

        let target_path = Path::new()
            .with_join_str("..")
            .with_join_str("..")
            .with_join(bin_path);

        workspace_nm_tree.register_entry(bin_symlink_path, SyncItem::Symlink {
            target_path,
        })?;
    }

    Ok(())
}

pub async fn link_project_nm(project: &Project, install: &Install) -> Result<LinkResult, Error> {
    let mut work_tree
        = WorkTree::new(project, &install.install_state);

    let mut hoister
        = Hoister::new(&mut work_tree);

    let mut packages_by_location
        = BTreeMap::new();

    hoister.hoist();

    let mut project_queue
        = vec![0usize];

    while let Some(workspace_node_idx) = project_queue.pop() {
        let workspace_node
            = &work_tree.nodes[workspace_node_idx];

        let workspace
            = project.workspace_by_locator(&workspace_node.locator)?;

        packages_by_location.insert(workspace.rel_path.clone(), workspace_node.locator.clone());

        let workspace_dir
            = project.project_cwd
                .with_join(&workspace.rel_path);

        let workspace_abs_path
            = workspace_dir
                .with_join_str("node_modules");

        let mut workspace_nm_tree
            = SyncTree::new();

        workspace_nm_tree.dry_run = false;

        let workspace_binaries
            = collect_workspace_binaries(install, &work_tree.nodes[workspace_node_idx]);

        register_workspace_bin_symlinks(&mut workspace_nm_tree, &workspace_dir, &workspace_binaries)?;

        let mut workspace_queue
            = vec![(Path::new(), workspace_node_idx)];

        while let Some((node_rel_path, node_idx)) = workspace_queue.pop() {
            let node
                = &work_tree.nodes[node_idx];

            let children
                = node.children.as_ref()
                    .expect(EXPECT_CHILDREN);

            for (ident, child_idx) in children {
                let child_node
                    = &work_tree.nodes[*child_idx];

                let child_rel_path
                    = node_rel_path.with_join_str(&ident.as_str());

                workspace_queue.push((child_rel_path.with_join_str("node_modules"), *child_idx));

                let abs_path
                    = workspace_abs_path
                        .with_join(&node_rel_path)
                        .with_join(&child_rel_path);

                let rel_path
                    = abs_path
                        .relative_to(&project.project_cwd);

                packages_by_location.insert(rel_path, child_node.locator.clone());

                let package_data
                    = install.package_data.get(&child_node.locator.physical_locator());

                match package_data {
                    Some(PackageData::Abstract) => {
                        return Err(Error::Unsupported);
                    },

                    Some(PackageData::Local {package_directory, ..}) => {
                        let child_abs_path
                            = workspace_abs_path.with_join(&child_rel_path);

                        let target_path
                            = package_directory.relative_to(&child_abs_path.dirname().unwrap());

                        workspace_nm_tree.register_entry(child_rel_path, SyncItem::Symlink {
                            target_path: target_path.clone(),
                        })?;
                    },

                    Some(PackageData::Zip {archive_path, package_directory, ..}) => {
                        workspace_nm_tree.register_entry(child_rel_path, SyncItem::Folder {
                            template: Some(SyncTemplate::Zip {
                                archive_path: archive_path.clone(),
                                inner_path: package_directory.relative_to(&archive_path),
                            }),
                        })?;
                    },

                    Some(PackageData::MissingZip {..}) => {
                        // Nothing to do here
                    },

                    None => match &child_node.locator.reference {
                        Reference::Link(params) if params.path.starts_with('/') => {
                            let target_path
                                = Path::from_file_string(&params.path)?;

                            workspace_nm_tree.register_entry(child_rel_path, SyncItem::Symlink {
                                target_path,
                            })?;
                        },

                        _ => {
                            unreachable!("Expected package data for {}", ToHumanString::to_print_string(&child_node.locator.physical_locator()));
                        },
                    },
                }
            }

            // Register bin symlinks for all direct dependencies of this node
            // Skip any bins that conflict with workspace binaries (workspace takes precedence)
            let mut binaries
                = collect_binaries_from_dependencies(install, children, &work_tree);

            // For root level, filter out bins that conflict with workspace binaries
            if node_rel_path.is_empty() {
                for bin_name in workspace_binaries.keys() {
                    binaries.remove(bin_name);
                }
            }

            register_bin_symlinks_at_path(&mut workspace_nm_tree, &node_rel_path, &binaries)?;
        }

        workspace_nm_tree
            .run(workspace_abs_path)?;

        project_queue.extend_from_slice(&workspace_node.workspaces_idx);
    }

    Ok(LinkResult {
        packages_by_location,
        build_requests: BuildRequests {
            entries: vec![],
            dependencies: BTreeMap::new(),
        },
    })
}
