use std::collections::BTreeSet;

use clipanion::cli;
use indexmap::IndexMap;
use itertools::Itertools;
use zpm_primitives::{DescriptorResolution, Ident, IdentGlob, Locator};
use zpm_utils::{AbstractValue, ToFileString, tree};

use crate::{
    error::Error,
    install::InstallState,
    project::Project,
};

/// Display the reason why a package is needed
///
/// This command prints the exact reasons why a package appears in the dependency tree.
///
/// By default, it will print all packages that directly depend on the specified package. If you want to see all transitive
/// paths that lead to the package (going through all workspaces), use the `-R,--recursive` flag.
///
/// Note that the recursive display is optimized to avoid printing the same package subtree multiple times. If you see a
/// package without children in one branch, it means its subtree was already printed elsewhere in the tree.
///
#[cli::command]
#[cli::path("why")]
#[cli::category("Dependency management")]
pub struct Why {
    /// List all dependency paths from each workspace
    #[cli::option("-R,--recursive", default = false)]
    recursive: bool,

    /// Format the output as an NDJSON stream
    #[cli::option("--json", default = false)]
    json: bool,

    /// Also print peer dependencies that match the specified name
    #[cli::option("--peers", default = false)]
    peers: bool,

    /// The package to search for
    pattern: IdentGlob,
}

impl Why {
    pub async fn execute(&self) -> Result<(), Error> {
        let mut project
            = Project::new(None).await?;

        project.lazy_install().await?;

        let install_state = project
            .install_state
            .as_ref()
            .ok_or(Error::InstallStateNotFound)?;

        let root_node = if self.recursive {
            self.why_recursive(&project, install_state)?
        } else {
            self.why_simple(install_state)?
        };

        let rendering
            = tree::TreeRenderer::new()
                .render(&root_node, self.json);

        print!("{}", rendering);

        Ok(())
    }

    fn why_simple(&self, install_state: &InstallState) -> Result<tree::Node<'_>, Error> {
        let mut root_children
            = vec![];

        let mut sorted_locators
            = install_state.resolution_tree.locator_resolutions.keys()
                .collect_vec();

        sorted_locators.sort();

        for locator in sorted_locators {
            let resolution
                = &install_state.resolution_tree.locator_resolutions[locator];

            let mut children_map
                = IndexMap::new();

            for (ident, descriptor) in &resolution.dependencies {
                if !self.peers && resolution.peer_dependencies.contains_key(ident) {
                    continue;
                }

                let dep_locator
                    = install_state.resolution_tree.descriptor_to_locator
                        .get(descriptor);

                if let Some(dep_locator) = dep_locator {
                    if self.pattern.check(&dep_locator.ident) {
                        let descriptor_resolution
                            = DescriptorResolution::new(descriptor.clone(), dep_locator.clone());

                        children_map.insert(
                            dep_locator.to_file_string(),
                            tree::Node::new_value(descriptor_resolution),
                        );
                    }
                }
            }

            if !children_map.is_empty() {
                root_children.push(tree::Node {
                    label: None,
                    value: Some(AbstractValue::new(locator.clone())),
                    children: Some(tree::TreeNodeChildren::Map(children_map)),
                });
            }
        }

        Ok(tree::Node {
            label: None,
            value: None,
            children: Some(tree::TreeNodeChildren::Vec(root_children)),
        })
    }

    fn why_recursive(&self, project: &Project, install_state: &InstallState) -> Result<tree::Node<'_>, Error> {
        let mut seen
            = BTreeSet::new();
        let mut dependents
            = BTreeSet::new();

        let target_idents: BTreeSet<Ident> = install_state
            .resolution_tree
            .locator_resolutions
            .keys()
            .filter(|locator| self.pattern.check(&locator.ident))
            .map(|locator| locator.ident.clone())
            .collect();

        if target_idents.is_empty() {
            return Ok(tree::Node {
                label: None,
                value: None,
                children: None,
            });
        }

        for workspace in &project.workspaces {
            self.mark_all_dependents(
                &workspace.locator(),
                install_state,
                &target_idents,
                &mut seen,
                &mut dependents,
            );
        }

        let mut printed
            = BTreeSet::new();
        let mut root_children
            = IndexMap::new();

        for workspace in &project.workspaces {
            let workspace_locator
                = workspace.locator();

            self.print_all_dependents(
                &workspace_locator,
                None,
                install_state,
                project,
                &dependents,
                &mut printed,
                &mut root_children,
            );
        }

        // Convert the map to a vec for the root node
        let root_children_vec
            = root_children.into_iter()
                .map(|(_, node)| node)
                .collect_vec();

        Ok(tree::Node {
            label: None,
            value: None,
            children: Some(tree::TreeNodeChildren::Vec(root_children_vec)),
        })
    }

    fn mark_all_dependents(&self, locator: &Locator, install_state: &InstallState, target_idents: &BTreeSet<Ident>, seen: &mut BTreeSet<Locator>, dependents: &mut BTreeSet<Locator>) -> bool {
        if seen.contains(locator) {
            return dependents.contains(locator);
        }

        seen.insert(locator.clone());

        if target_idents.contains(&locator.ident) {
            dependents.insert(locator.clone());
            return true;
        }

        let resolution = match install_state.resolution_tree.locator_resolutions.get(locator) {
            Some(res) => res,
            None => return false,
        };

        let mut depends
            = false;

        for (ident, descriptor) in &resolution.dependencies {
            if !self.peers && resolution.peer_dependencies.contains_key(ident) {
                continue;
            }

            let dep_locator =
                install_state.resolution_tree.descriptor_to_locator.get(descriptor);

            if let Some(dep_locator) = dep_locator {
                if self.mark_all_dependents(dep_locator, install_state, target_idents, seen, dependents) {
                    depends = true;
                }
            }
        }

        if depends {
            dependents.insert(locator.clone());
        }

        depends
    }

    fn print_all_dependents(&self, locator: &Locator, descriptor: Option<&zpm_primitives::Descriptor>, install_state: &InstallState, project: &Project, dependents: &BTreeSet<Locator>, printed: &mut BTreeSet<Locator>, parent_children: &mut IndexMap<String, tree::Node<'_>>) {
        if !dependents.contains(locator) {
            return;
        }

        let resolution
            = install_state.resolution_tree.locator_resolutions
                .get(locator)
                .expect("Locator not found in resolution tree; where does it come from?");

        let is_workspace
            = project.workspaces.iter()
                .any(|ws| ws.locator() == *locator);

        let mut node_children
            = IndexMap::new();

        let should_print_children = (!printed.contains(locator) || is_workspace)
            && !(descriptor.is_some() && is_workspace);

        if should_print_children {
            printed.insert(locator.clone());

            for (ident, dep_descriptor) in &resolution.dependencies {
                if !self.peers && resolution.peer_dependencies.contains_key(ident) {
                    continue;
                }

                let dep_locator
                    = install_state.resolution_tree.descriptor_to_locator
                        .get(dep_descriptor);

                if let Some(dep_locator) = dep_locator {
                    self.print_all_dependents(dep_locator, Some(dep_descriptor), install_state, project, dependents, printed, &mut node_children);
                }
            }
        }

        let node_value = if let Some(desc) = descriptor {
            AbstractValue::new(DescriptorResolution::new(desc.clone(), locator.clone()))
        } else {
            AbstractValue::new(locator.clone())
        };

        parent_children.insert(
            locator.to_file_string(),
            tree::Node {
                label: None,
                value: Some(node_value),
                children: Some(tree::TreeNodeChildren::Map(node_children)),
            },
        );
    }
}
