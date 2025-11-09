use std::{collections::BTreeMap, marker::PhantomData};

use itertools::Itertools;
use zpm_primitives::Locator;
use zpm_utils::Path;

use crate::{
    build::BuildRequests, error::Error, fetchers::PackageData, graph::{GraphIn, GraphOut}, install::Install, linker::{self, LinkResult, nm::hoist::{Hoister, WorkTree}}, project::Project
};

pub mod hoist;

const EXPECT_CHILDREN: &str = "All nodes should be expanded by the end of the hoisting process";

struct LinkContext<'a> {
    install: &'a Install,
    work_tree: &'a WorkTree<'a>,
}

enum LinkOp<'a> {
    #[allow(dead_code)]
    Phantom(PhantomData<&'a ()>),

    CreateContainer {
        node_idx: usize,
        path: Path,
    },

    ExtractPackage {
        locator: Locator,
        path: Path,
    },
}

enum LinkOpResult {
    ContainerCreated {
        node_idx: usize,
        path: Path,
    },

    PackageExtracted,
}

impl<'a> GraphOut<LinkContext<'a>, LinkOp<'a>> for LinkOpResult {
    fn graph_follow_ups(&self, ctx: &LinkContext<'a>) -> Vec<LinkOp<'a>> {
        match self {
            LinkOpResult::ContainerCreated {node_idx, path} => {
                let node
                    = &ctx.work_tree.nodes[*node_idx];

                let children = node.children
                    .as_ref()
                    .expect(EXPECT_CHILDREN);

                let nm_ops = children.values()
                    .map(|child_idx| (*child_idx, &ctx.work_tree.nodes[*child_idx]))
                    .filter(|(_, child_node)| !child_node.children.as_ref().expect(EXPECT_CHILDREN).is_empty())
                    .map(|(child_idx, child_node)| LinkOp::CreateContainer {node_idx: child_idx, path: path.with_join(&child_node.locator.ident.nm_subdir())});

                let extract_ops = children.values()
                    .map(|child_idx| ctx.work_tree.nodes[*child_idx].locator.physical_locator())
                    .flat_map(|child_locator| ctx.install.package_data.get(&child_locator).map(|package_data| (child_locator, package_data)))
                    .filter(|(_, package_data)| matches!(package_data, PackageData::Zip {..}))
                    .map(|(child_locator, _)| LinkOp::ExtractPackage {locator: child_locator.clone(), path: path.with_join(&child_locator.ident.nm_subdir())});

                nm_ops
                    .chain(extract_ops)
                    .collect_vec()
            },

            LinkOpResult::PackageExtracted => {
                vec![]
            },
        }
    }
}

impl<'a> GraphIn<'a, LinkContext<'a>, LinkOpResult, Error> for LinkOp<'a> {
    fn graph_dependencies(&self, _ctx: &LinkContext<'a>, _resolved_dependencies: &[&LinkOpResult]) -> Vec<Self> {
        vec![]
    }

    async fn graph_run(self, context: LinkContext<'a>, _dependencies: Vec<LinkOpResult>) -> Result<LinkOpResult, Error> {
        match self {
            LinkOp::Phantom(_) =>
                unreachable!("PhantomData should never be instantiated"),

            LinkOp::CreateContainer {node_idx, path} => {
                let node
                    = &context.work_tree.nodes[node_idx];

                let children
                    = node.children.as_ref()
                        .expect(EXPECT_CHILDREN);

                if !children.is_empty() {
                    let nm_path
                        = path.with_join_str("node_modules");

                    nm_path.fs_create_dir();

                    for &child_idx in children.values() {
                        let child_node
                            = &context.work_tree.nodes[child_idx];

                        let (scope, name)
                            = child_node.locator.ident.split();

                        let mut current
                            = nm_path.clone();

                        if let Some(scope) = scope {
                            current.join_str(scope);
                            current.fs_create_dir();
                        }

                        let physical_locator
                            = child_node.locator.physical_locator();

                        let package_data
                            = &context.install.package_data[&physical_locator];

                        if let PackageData::Local {package_directory, ..} = package_data {
                            let relative_path
                                = package_directory.relative_to(&current);

                            current.join_str(name);
                            current.fs_symlink(&relative_path);
                        } else {
                            current.join_str(name);
                            current.fs_create_dir();

                            let has_own_children
                                = !child_node.children.as_ref()
                                    .expect(EXPECT_CHILDREN)
                                    .is_empty();

                            if has_own_children {
                                current.join_str("node_modules");
                                current.fs_create_dir();
                            }
                        }
                    }
                }

                Ok(LinkOpResult::ContainerCreated {node_idx, path})
            },

            LinkOp::ExtractPackage {locator, path} => {
                let physical_locator
                    = locator.physical_locator();

                let physical_package_data
                    = &context.install.package_data[&physical_locator];

                linker::helpers::fs_extract_archive(
                    &path,
                    physical_package_data,
                )?;

                Ok(LinkOpResult::PackageExtracted)
            },
        }
    }
}


pub async fn link_project_nm(project: &Project, install: &Install) -> Result<LinkResult, Error> {
    let mut work_tree
        = WorkTree::new(project, &install.install_state);

    let mut hoister
        = Hoister::new(&mut work_tree);

    hoister.hoist();

    let mut workspace_queue
        = vec![0usize];

    while let Some(node_idx) = workspace_queue.pop() {
        let node
            = &work_tree.nodes[node_idx];

        workspace_queue.extend_from_slice(&node.workspaces_idx);
    }

    Ok(LinkResult {
        packages_by_location: BTreeMap::new(),
        build_requests: BuildRequests {
            entries: vec![],
            dependencies: BTreeMap::new(),
        },
    })
}
