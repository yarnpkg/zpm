use std::collections::BTreeSet;

use clipanion::cli;
use indexmap::IndexMap;
use itertools::Itertools;
use zpm_primitives::{DescriptorResolution, Locator, VersionFilter};
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
/// The package can be followed by a semver range (`<package>@<range>`), in which case only the instances of the package whose resolved version
/// satisfies the range are reported. This is particularly useful when trying to find out why your project depends on older versions of a package.
/// The range must be a valid semver range; protocols (`npm:^1.0.0`) and tags (`latest`) aren't accepted. The package name itself can be a glob
/// pattern (`@babel/*`).
///
/// By default, it will print all packages that directly depend on the specified package. If you want to see all transitive
/// paths that lead to the package (going through all workspaces), use the `-R,--recursive` flag.
///
/// Note that the recursive display is optimized to avoid printing the same package subtree multiple times. If you see a
/// package without children in one branch, it means its subtree was already printed elsewhere in the tree.
///
/// ## Examples
///
/// Explain why lodash is used in your project
///
/// `yarn why lodash`
///
/// Explain why version 3.3.1 of lodash is in your project
///
/// `yarn why lodash@3.3.1`
///
/// Explain why versions 3.x of lodash are in your project
///
/// `yarn why lodash@^3`
///
/// Print all the dependency paths that lead to lodash
///
/// `yarn why lodash -R`
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

    /// The package to search for, optionally followed by a semver range
    pattern: VersionFilter,
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
            self.why_simple(&project, install_state)?
        };

        let rendering
            = tree::TreeRenderer::new()
                .render(&root_node, self.json);

        print!("{}", rendering);

        Ok(())
    }

    fn is_target(&self, locator: &Locator, install_state: &InstallState) -> bool {
        install_state.resolution_tree.locator_resolutions
            .get(locator)
            .is_some_and(|resolution| self.pattern.check(&locator.ident, &resolution.version))
    }

    fn why_simple(&self, project: &Project, install_state: &InstallState) -> Result<tree::Node<'_>, Error> {
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
                    if self.is_target(dep_locator, install_state) {
                        let display_dep_locator
                            = project.displayable_locator(dep_locator);

                        let descriptor_resolution
                            = DescriptorResolution::new(descriptor.clone(), display_dep_locator.clone());

                        children_map.insert(
                            display_dep_locator.to_file_string(),
                            tree::Node::new_value(descriptor_resolution),
                        );
                    }
                }
            }

            if !children_map.is_empty() {
                let display_locator = project.displayable_locator(locator);

                root_children.push(tree::Node {
                    label: None,
                    value: Some(AbstractValue::new(display_locator)),
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

        let targets: BTreeSet<Locator> = install_state
            .resolution_tree
            .locator_resolutions
            .keys()
            .filter(|locator| self.is_target(locator, install_state))
            .cloned()
            .collect();

        if targets.is_empty() {
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
                &targets,
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

    fn mark_all_dependents(&self, locator: &Locator, install_state: &InstallState, targets: &BTreeSet<Locator>, seen: &mut BTreeSet<Locator>, dependents: &mut BTreeSet<Locator>) -> bool {
        if seen.contains(locator) {
            return dependents.contains(locator);
        }

        seen.insert(locator.clone());

        if targets.contains(locator) {
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
                if self.mark_all_dependents(dep_locator, install_state, targets, seen, dependents) {
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

        let display_locator = project.displayable_locator(locator);

        let node_value = if let Some(desc) = descriptor {
            AbstractValue::new(DescriptorResolution::new(desc.clone(), display_locator.clone()))
        } else {
            AbstractValue::new(display_locator.clone())
        };

        parent_children.insert(
            display_locator.to_file_string(),
            tree::Node {
                label: None,
                value: Some(node_value),
                children: Some(tree::TreeNodeChildren::Map(node_children)),
            },
        );
    }
}

