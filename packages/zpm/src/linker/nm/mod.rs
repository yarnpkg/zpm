use std::collections::BTreeMap;

use zpm_primitives::Reference;
use zpm_sync::{SyncItem, SyncTemplate, SyncTree};
use zpm_utils::{FromFileString, Path, ToHumanString};

use crate::{
    build::BuildRequests, error::Error, fetchers::PackageData, install::Install, linker::{LinkResult, nm::hoist::{Hoister, WorkTree}}, project::Project
};

pub mod hoist;

const EXPECT_CHILDREN: &str = "All nodes should be expanded by the end of the hoisting process";

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

        let workspace_abs_path
            = project.project_cwd
                .with_join(&workspace.rel_path)
                .with_join_str("node_modules");

        let mut workspace_nm_tree
            = SyncTree::new();

        workspace_nm_tree.dry_run = false;

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
