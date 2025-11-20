use std::collections::{BTreeMap, BTreeSet};

use clipanion::cli;
use indexmap::IndexMap;
use zpm_primitives::{DescriptorResolution, Ident, IdentGlob, Locator};
use zpm_utils::{tree, AbstractValue, ToFileString};

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
#[cli::usage(r#"
This command prints the exact reasons why a package appears in the dependency tree.

By default, it will print all packages that directly depend on the specified package. If you want to see all transitive paths that lead to the package (going through all workspaces), use the `-R,--recursive` flag.

Note that the recursive display is optimized to avoid printing the same package subtree multiple times. If you see a package without children in one branch, it means its subtree was already printed elsewhere in the tree.
"#)]
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
        let mut project = Project::new(None).await?;

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

        let rendering = tree::TreeRenderer::new().render(&root_node, self.json);

        print!("{}", rendering);

        Ok(())
    }

    fn why_simple(&self, install_state: &InstallState) -> Result<tree::Node<'_>, Error> {
        let mut root_children = vec![];

        // Sort locators for deterministic output
        let mut sorted_locators: Vec<_> = install_state
            .resolution_tree
            .locator_resolutions
            .keys()
            .collect();
        sorted_locators.sort();

        for locator in sorted_locators {
            let resolution = &install_state.resolution_tree.locator_resolutions[locator];
            let mut children_map = IndexMap::new();

            for (ident, descriptor) in &resolution.dependencies {
                // Skip peer dependencies unless --peers flag is set
                if !self.peers && resolution.peer_dependencies.contains_key(ident) {
                    continue;
                }

                // Get the resolved locator for this dependency
                let dep_locator = install_state
                    .resolution_tree
                    .descriptor_to_locator
                    .get(descriptor);

                if let Some(dep_locator) = dep_locator {
                    // Check if this dependency matches our search pattern
                    if self.pattern.check(&dep_locator.ident) {
                        // Create a DescriptorResolution to match Berry's format
                        let descriptor_resolution = DescriptorResolution::new(
                            descriptor.clone(),
                            dep_locator.clone(),
                        );

                        // Use the locator as the key (e.g., "react@npm:18.3.1")
                        let key = dep_locator.to_file_string();
                        children_map.insert(
                            key,
                            tree::Node {
                                label: None,
                                value: Some(AbstractValue::new(descriptor_resolution)),
                                children: None,
                            },
                        );
                    }
                }
            }

            // Only add this locator to the tree if it has matching dependencies
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

    fn why_recursive(
        &self,
        project: &Project,
        install_state: &InstallState,
    ) -> Result<tree::Node<'_>, Error> {
        // Phase 1: Mark all packages that depend (directly or transitively) on the target
        let mut seen = BTreeSet::new();
        let mut dependents = BTreeSet::new();

        // Get all matching target locators
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

        // Start from all workspaces
        for workspace in &project.workspaces {
            self.mark_all_dependents(
                &workspace.locator(),
                install_state,
                &target_idents,
                &mut seen,
                &mut dependents,
            );
        }

        // Phase 2: Build the tree by printing all dependents
        let mut printed = BTreeSet::new();
        let mut root_children = vec![];

        for workspace in &project.workspaces {
            let workspace_locator = workspace.locator();
            self.print_all_dependents(
                &workspace_locator,
                None,  // Root workspaces don't have a descriptor
                install_state,
                project,
                &dependents,
                &mut printed,
                &mut root_children,
            );
        }

        Ok(tree::Node {
            label: None,
            value: None,
            children: Some(tree::TreeNodeChildren::Vec(root_children)),
        })
    }

    fn mark_all_dependents(
        &self,
        locator: &Locator,
        install_state: &InstallState,
        target_idents: &BTreeSet<Ident>,
        seen: &mut BTreeSet<Locator>,
        dependents: &mut BTreeSet<Locator>,
    ) -> bool {
        // If already processed, return whether it's a dependent
        if seen.contains(locator) {
            return dependents.contains(locator);
        }

        seen.insert(locator.clone());

        // Check if this locator is itself a target
        if target_idents.contains(&locator.ident) {
            dependents.insert(locator.clone());
            return true;
        }

        let resolution = match install_state.resolution_tree.locator_resolutions.get(locator) {
            Some(res) => res,
            None => return false,
        };

        let mut depends = false;

        // Check all dependencies
        for (ident, descriptor) in &resolution.dependencies {
            // Skip peer dependencies unless --peers flag is set
            if !self.peers && resolution.peer_dependencies.contains_key(ident) {
                continue;
            }

            if let Some(dep_locator) =
                install_state.resolution_tree.descriptor_to_locator.get(descriptor)
            {
                if self.mark_all_dependents(
                    dep_locator,
                    install_state,
                    target_idents,
                    seen,
                    dependents,
                ) {
                    depends = true;
                }
            }
        }

        if depends {
            dependents.insert(locator.clone());
        }

        depends
    }

    fn print_all_dependents(
        &self,
        locator: &Locator,
        descriptor: Option<&zpm_primitives::Descriptor>,
        install_state: &InstallState,
        project: &Project,
        dependents: &BTreeSet<Locator>,
        printed: &mut BTreeSet<Locator>,
        parent_children: &mut Vec<tree::Node<'_>>,
    ) {
        // Only print if this is a dependent
        if !dependents.contains(locator) {
            return;
        }

        let resolution = match install_state.resolution_tree.locator_resolutions.get(locator) {
            Some(res) => res,
            None => return,
        };

        // Check if this is a workspace by looking for it in project.workspaces
        let is_workspace = project
            .workspaces
            .iter()
            .any(|ws| ws.locator() == *locator);

        let mut node_children = vec![];

        // If already printed and not a workspace root, skip printing children
        // Don't print children of transitive workspace dependencies
        let should_print_children = (!printed.contains(locator) || is_workspace)
            && !(descriptor.is_some() && is_workspace);

        if should_print_children {
            printed.insert(locator.clone());

            // Print dependencies that are also dependents
            for (ident, dep_descriptor) in &resolution.dependencies {
                // Skip peer dependencies unless --peers flag is set
                if !self.peers && resolution.peer_dependencies.contains_key(ident) {
                    continue;
                }

                if let Some(dep_locator) =
                    install_state.resolution_tree.descriptor_to_locator.get(dep_descriptor)
                {
                    self.print_all_dependents(
                        dep_locator,
                        Some(dep_descriptor),
                        install_state,
                        project,
                        dependents,
                        printed,
                        &mut node_children,
                    );
                }
            }
        }

        // Create the node value based on whether this has a descriptor (non-root) or not (root)
        let node_value = if let Some(desc) = descriptor {
            // Non-root nodes use DescriptorResolution
            AbstractValue::new(DescriptorResolution::new(desc.clone(), locator.clone()))
        } else {
            // Root nodes (workspaces) use just the Locator
            AbstractValue::new(locator.clone())
        };

        let node = tree::Node {
            label: None,
            value: Some(node_value),
            children: if node_children.is_empty() {
                None
            } else {
                Some(tree::TreeNodeChildren::Vec(node_children))
            },
        };

        parent_children.push(node);
    }
}

#[cfg(test)]
#[path = "why.test.rs"]
mod tests;
