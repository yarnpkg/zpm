use std::collections::{BTreeMap, BTreeSet};

use itertools::Itertools;
use zpm_primitives::{Ident, LinkReference, Locator, Reference};
use zpm_utils::{Path, ToFileString, ToHumanString, tree};

use crate::{
    algos,
    install::InstallState,
    project::Project,
};

fn convert_workspace_to_link(project: &Project, locator: Locator) -> Locator {
    let physical_locator
        = locator.physical_locator();

    if physical_locator.reference.is_workspace_reference() {
        let workspace_location
            = project.package_location(&physical_locator)
                .expect("Expected the workspace to have a package location");

        Locator::new(locator.ident.clone(), LinkReference {
            path: workspace_location.to_file_string(),
        }.into())
    } else {
        locator
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct WorkNode {
    pub parent_idx: Option<usize>,
    pub workspaces_idx: Vec<usize>,
    pub locator: Locator,
    pub dependencies: BTreeMap<Ident, Locator>,
    pub children: Option<BTreeMap<Ident, usize>>,
    pub updated: bool,
}

pub struct WorkTree<'a> {
    pub project: &'a Project,
    pub install_state: &'a InstallState,
    pub nodes: Vec<WorkNode>,
}

impl<'a> WorkTree<'a> {
    pub fn new(project: &'a Project, install_state: &'a InstallState) -> Self {
        let mut tree = Self {
            project,
            install_state,
            nodes: vec![],
        };

        let root_idx
            = tree.create_node(project.root_workspace().locator(), None, false);

        tree.expand_node(root_idx);
        tree.import_workspaces(root_idx);

        tree
    }

    fn import_workspaces(&mut self, root_idx: usize) {
        let mut workspace_path_set
            = BTreeSet::new();

        for workspace in &self.project.workspaces {
            workspace_path_set.insert(&workspace.rel_path);
        }

        let mut edges: BTreeMap<_, Vec<_>>
            = BTreeMap::new();

        for workspace in &self.project.workspaces {
            let parent = workspace.rel_path.iter_path().rev()
                .skip(1)
                .find(|path| workspace_path_set.contains(path));

            if let Some(parent) = parent {
                edges.entry(parent).or_default().push(workspace);
            }
        }

        let root_path
            = Path::new();

        let mut queue
            = vec![(root_idx, &root_path)];

        while let Some((parent_idx, prefix_path)) = queue.pop() {
            let inner_workspaces
                = edges.get(&prefix_path);

            if let Some(inner_workspaces) = inner_workspaces {
                for inner_workspace in inner_workspaces {
                    let inner_workspace_locator
                        = inner_workspace.locator();

                    let inner_workspace_idx
                        = self.create_node(inner_workspace_locator, Some(parent_idx), false);

                    queue.push((inner_workspace_idx, &inner_workspace.rel_path));

                    self.nodes[parent_idx].workspaces_idx
                        .push(inner_workspace_idx);
                }
            }
        }
    }

    fn create_node(&mut self, locator: Locator, parent_idx: Option<usize>, terminal_workspaces: bool) -> usize {
        let mut dependencies
            = BTreeMap::new();
        let mut children
            = None;

        let physical_locator
            = locator.physical_locator();

        let may_have_dependencies
            = (!physical_locator.reference.is_workspace_reference() || !terminal_workspaces) && !matches!(physical_locator.reference, Reference::Link(_));

        if may_have_dependencies {
            let resolution
                = &self.install_state.resolution_tree.locator_resolutions[&locator];

            dependencies = resolution.dependencies.iter()
                .map(|(ident, descriptor)| (ident.clone(), self.install_state.resolution_tree.descriptor_to_locator[descriptor].clone()))
                .collect();
        } else {
            children = Some(BTreeMap::new());
        }

        let node = WorkNode {
            parent_idx,
            workspaces_idx: vec![],
            locator,
            dependencies,
            children,
            updated: true,
        };

        let node_idx = self.nodes.len();
        self.nodes.push(node);

        node_idx
    }

    fn expand_node(&mut self, node_idx: usize) {
        let node
            = &self.nodes[node_idx];

        if node.children.is_some() {
            return;
        }

        let mut parent_chain
            = BTreeSet::new();
        let mut parent_queue
            = vec![node_idx];

        while let Some(parent_idx) = parent_queue.pop() {
            let parent_node
                = &self.nodes[parent_idx];

            parent_chain.insert(parent_node.locator.clone());
            parent_queue.extend(parent_node.parent_idx.iter().copied());
        }

        let peer_dependencies
            = &self.install_state.resolution_tree.locator_resolutions[&node.locator].peer_dependencies;

        let children
            = node.dependencies.clone()
                .into_iter()
                .filter(|(_, dependency)| !peer_dependencies.contains_key(&dependency.ident))
                .filter(|(_, dependency)| !parent_chain.contains(&dependency))
                .map(|(ident, dependency)| (ident, convert_workspace_to_link(self.project, dependency)))
                .map(|(ident, dependency)| (ident, self.create_node(dependency, Some(node_idx), true)))
                .collect::<BTreeMap<_, _>>();

        let node
            = &mut self.nodes[node_idx];

        node.children = Some(children);
        node.updated = false;
    }
}

pub struct TreeRenderer<'a, 'b> {
    tree: &'a WorkTree<'b>,
    parent_stack: Vec<usize>,
}

impl<'a, 'b> TreeRenderer<'a, 'b> {
    pub fn new(tree: &'a WorkTree<'b>) -> Self {
        Self {
            tree,
            parent_stack: vec![],
        }
    }

    pub fn convert(&mut self) -> tree::Node<'a> {
        self.convert_impl(0, BTreeMap::new())
    }

    fn convert_impl(&mut self, node_idx: usize, available_dependencies: BTreeMap<&'a Ident, &'a Locator>) -> tree::Node<'a> {
        let is_cycle
            = self.parent_stack.contains(&node_idx);

        self.parent_stack.push(node_idx);

        let available_dependencies
            = self.extend_available_dependencies(available_dependencies, node_idx);

        let node
            = &self.tree.nodes[node_idx];

        let is_workspace_link
            = |expected_locator: &Locator, locator: &Locator|
                expected_locator.reference.is_workspace_reference() && locator.ident == expected_locator.ident && locator.reference == LinkReference {
                    path: self.tree.project.package_location(&expected_locator).unwrap().to_file_string(),
                }.into();

        let is_dependency_valid
            = |expected_locator: &Locator|
                available_dependencies.get(&expected_locator.ident)
                    .map_or(false, |&available_locator| available_locator == expected_locator || is_workspace_link(expected_locator, available_locator));

        let invalid_dependencies
            = node.dependencies.values()
                .filter(|&expected_locator| !is_dependency_valid(expected_locator))
                .collect_vec();

        let mut label
            = node.locator.to_print_string();

        if is_cycle {
            label.push_str(" (cycle)");
        }

        let mut tree_children
            = Vec::new();

        for &locator in &invalid_dependencies {
            let invalid_dependency_locator
                = available_dependencies.get(&locator.ident);

            let label = format!(
                "❌ Invalid dependency {} (expected {})",
                invalid_dependency_locator.map(|locator| locator.to_print_string()).unwrap_or_else(|| "<none>".to_string()),
                locator.to_print_string(),
            );

            tree_children.push(tree::Node {
                label: Some(label),
                value: None,
                children: None,
            });
        }

        let node_children
            = node.children
                .as_ref()
                .expect("Expected the children to be present since we just expanded this node")
                .values()
                .chain(node.workspaces_idx.iter())
                .copied()
                .collect_vec();

        for child_idx in node_children {
            tree_children.push(self.convert_impl(child_idx, available_dependencies.clone()));
        }

        self.parent_stack.pop();

        tree::Node {
            label: Some(label),
            value: None,
            children: Some(tree::TreeNodeChildren::Vec(tree_children)),
        }
    }

    fn extend_available_dependencies(&self, mut available_dependencies: BTreeMap<&'a Ident, &'a Locator>, node_idx: usize) -> BTreeMap<&'a Ident, &'a Locator> {
        let node
            = &self.tree.nodes[node_idx];

        available_dependencies.insert(&node.locator.ident, &node.locator);

        available_dependencies.extend(node.children.as_ref().unwrap().iter().map(|(ident, child_idx)| {
            let child_node
                = &self.tree.nodes[*child_idx];

            (ident, &child_node.locator)
        }));

        available_dependencies
    }
}

pub struct Hoister<'a, 'b> {
    work_tree: &'a mut WorkTree<'b>,
    stack: Vec<usize>,
    has_changed: bool,
    print_logs: bool,
}

impl<'a, 'b> Hoister<'a, 'b> {
    pub fn new(work_tree: &'a mut WorkTree<'b>) -> Self {
        Self {
            work_tree,
            stack: vec![],
            has_changed: false,
            print_logs: false,
        }
    }

    pub fn set_print_logs(&mut self, print_logs: bool) {
        self.print_logs = print_logs;
    }

    pub fn hoist(&mut self) {
        self.stack.clear();

        self.has_changed = true;

        while self.has_changed {
            if self.print_logs {
                self.print("=== Hoisting pass ===\n");
            }

            self.has_changed = false;
            self.process_node(0);
        }
    }

    fn print(&self, message: &str) {
        println!("{}{}", "  ".repeat(self.stack.len()), message);
    }

    fn process_node(&mut self, node_idx: usize) {
        if self.print_logs {
            self.print(&format!(
                "Attempting to hoist dependencies into {}",
                self.work_tree.nodes[node_idx].locator.to_print_string(),
            ));
        }

        //self.seen[node_idx] = true;
        self.stack.push(node_idx);

        self.work_tree.expand_node(node_idx);

        let node
            = &self.work_tree.nodes[node_idx];

        let node_children
            = node.children
                .as_ref()
                .expect("Expected the children to be present since we just expanded this node")
                .values()
                .chain(node.workspaces_idx.iter())
                .copied()
                .collect_vec();

        let mut hoist_candidates_with_parents: BTreeMap<Locator, Vec<(usize, Ident, usize)>>
            = BTreeMap::new();

        for &child_idx in node_children.iter() {
            self.work_tree.expand_node(child_idx);

            let flattened_node
                = &self.work_tree.nodes[child_idx];

            let transitive_children
                = flattened_node.children.clone().unwrap();

            for (child_ident, &transitive_node) in transitive_children.iter() {
                let parents = hoist_candidates_with_parents
                    .entry(self.work_tree.nodes[transitive_node].locator.clone())
                        .or_default();

                self.work_tree.expand_node(transitive_node);

                parents.push((child_idx, child_ident.clone(), transitive_node));
            }
        }

        // The recursive hoisting may have caused our child packages to depend on copies of a package
        // we already have in our own children. In that case they can always be hoisted into us (since
        // they're already there!), so we remove them from the hoist candidates. This way they won't
        // be stuck if they are part of a SCC that somehow cannot be hoisted.

        let (locators_to_meld, hoist_candidates_with_parents): (Vec<_>, Vec<_>)
            = hoist_candidates_with_parents.into_iter()
                .partition(|(transitive_locator, _)| {
                    let existing_child_idx
                        = self.work_tree.nodes[node_idx].children.as_ref().unwrap()
                            .get(&transitive_locator.ident);

                    let existing_child_locator = existing_child_idx
                        .map(|&idx| &self.work_tree.nodes[idx].locator);

                    existing_child_locator == Some(transitive_locator)
                });

        for (_transitive_locator, parents) in locators_to_meld {
            for (parent_idx, child_ident, _child_node_idx) in parents {
                self.work_tree.nodes[parent_idx].children.as_mut().unwrap().remove(&child_ident).unwrap();
            }
        }

        if !hoist_candidates_with_parents.is_empty() {
            let node
                = &self.work_tree.nodes[node_idx];

            // At this point `hoist_original_parents` may contain multiple locators for the same ident, for
            // example if we have a package A that depends on B@1.0.0, and a package B that depends on
            // B@2.0.0. We need to filter that out to only keep the one locator that has the most
            // parents for each ident.

            let selected_hoist_candidates
                = hoist_candidates_with_parents.into_iter()
                    .chunk_by(|(locator, _)| locator.ident.clone())
                    .into_iter()
                    .map(|(_, it)| it.max_by_key(|(_, parents)| parents.len()).unwrap())
                    .collect::<BTreeMap<_, _>>();

            // We're going to sort those hoistable dependencies so we first hoist those that don't depend
            // on other dependencies which would also be hoisted in the same pass. This ensures that if
            // one of those hoisting candidate fails, we don't try to hoist the other dependencies that
            // depend on it.

            let mut hoisting_dependencies
                = BTreeMap::new();

            for (hoistable_locator, parents) in &selected_hoist_candidates {
                // We can take the node from any parent; we just need to read the locator dependencies
                // anyway (perhaps we should keep that elsewhere so we don't need to clone it?).
                let (_parent_idx, _child_ident, hoistable_idx)
                    = &parents[0];

                let hoistable_node
                    = &self.work_tree.nodes[*hoistable_idx];

                let mut candidate_dependencies
                    = BTreeSet::new();

                for dependency_locator in hoistable_node.dependencies.values() {
                    // This node already embed its own copy of its dependency, so we don't care about whatever
                    // else could conflict with the parent.
                    if hoistable_node.children.as_ref().unwrap().contains_key(&dependency_locator.ident) {
                        continue;
                    }

                    candidate_dependencies.insert(dependency_locator);
                }

                hoisting_dependencies.insert(hoistable_locator, candidate_dependencies);
            }

            // Now that we now which dependencies depend on which others, we can sort them in strongly
            // connected components (SCCs). All dependencies in a SCC unit must be hoisted together - if
            // one of them fails, all of them will be skipped. This happens very rarely, only if a package
            // A depends on package B, and package B depends on package A.

            let sccs
                = algos::scc_tarjan_pearce(&hoisting_dependencies);

            let mut new_dependencies
                = self.work_tree.nodes[node_idx].dependencies.clone();
            let mut new_children
                = self.work_tree.nodes[node_idx].children.clone().unwrap();

            // We can now start attempting to hoist the dependencies, set by set. For each set
            // we check the dependencies of each package in the set, making sure that they are
            // still fulfilled by what will be the new set of hoisted dependencies.

            let mut hoisted_dependencies_to_remove
                = vec![];

            'next_scc: for scc in &sccs {
                let mut scc_hoisting_requirements
                    = vec![];

                for &package in scc {
                    // We computed the inter-dependencies without taking into account the dependencies
                    // we would prefer to hoist. As a result the SCCs produced may reference packages
                    // that were not selected. Should that happen, we fail the hoisting attempt for
                    // the whole SCC.
                    let Some(package_dependencies) = hoisting_dependencies.get(package) else {
                        continue 'next_scc;
                    };

                    scc_hoisting_requirements.push((package, package_dependencies));
                }

                // The SCC cannot be hoisted if the dependency is a workspace. Note that we will only
                // ever encounter this situation for workspaces under their top-level view - if a
                // package depends on a workspace, we will have turned that dependency into a link
                // prior to hoisting.

                let is_workspace
                    = scc.iter().any(|&scc_locator| {
                        scc_locator.reference.is_workspace_reference()
                    });

                if is_workspace {
                    if self.print_logs {
                        self.print(&format!(
                            "Cannot hoist {} into {} because it's a workspace",
                            scc.iter().map(|locator| locator.to_print_string()).join(", "),
                            node.locator.to_print_string(),
                        ));
                    }

                    continue;
                }

                // Here we make sure that hoisting the SCC wouldn't break the parent's self
                // dependency. This may happen in very rare cases where a package A@1 depends on
                // package B which depends on a different version of package A@2. In that case we
                // could end up hoisting A@2 into B then A@1, breaking A@1's self-dependency.

                let coherent_parent
                    = scc.iter().all(|&scc_locator| {
                        scc_locator.ident != node.locator.ident || scc_locator == &node.locator
                    });

                if !coherent_parent {
                    if self.print_logs {
                        self.print(&format!(
                            "Cannot hoist {} into {} because it'd break the parent's self dependency",
                            scc.iter().map(|locator| locator.to_print_string()).join(", "),
                            node.locator.to_print_string(),
                        ));
                    }

                    continue;
                }

                // In this section we check if the entries of the SCC are compatible with
                // the parent node dependencies. They are compatible in the following cases:
                //
                // - The parent node doesn't have these entries in its dependencies; in that case
                //   we can just hoist them.
                //
                // - The parent node has exactly the same entry in its dependencies; in that case
                //   the requirement will be fulfilled even after hoisting the SCC.
                //
                // - The parent node has a different entry in its dependencies that shares the
                //   same physical locator. In that case we optimistically allow merging the two
                //   entries, hoping that the dependencies of those two entries are compatible. We
                //   don't check it here, we rely on the follow-up requirement check to confirm it.

                let mut dependency_conflicts
                    = scc.iter()
                        .filter(|&&scc_locator| new_dependencies.get(&scc_locator.ident).map_or(false, |existing_locator| existing_locator != scc_locator))
                        .peekable();

                if dependency_conflicts.peek().is_some() {
                    if self.print_logs {
                        self.print(&format!(
                            "Cannot hoist {} into {} because the former conflicts with the dependencies of the latter",
                            scc.iter().map(|locator| locator.to_print_string()).join(", "),
                            node.locator.to_print_string(),
                        ));

                        while let Some(dependency_conflict) = dependency_conflicts.next() {
                            let existing_locator
                                = new_dependencies.get(&dependency_conflict.ident)
                                    .expect("Expected the dependency to be present since otherwise there would be no conflict");

                            self.print(&format!(
                                "  - {} required {} but parent requires {} instead",
                                node.locator.to_print_string(), dependency_conflict.to_print_string(), existing_locator.to_print_string(),
                            ));
                        }
                    }

                    continue;
                }

                // We check here that the parent node doesn't already host children with the same
                // name as the entries in the SCC, or that they are identical.
                //
                // We also tolerate other virtual instances in the same package in this check, as
                // their requirements will be checked in the very next step.

                let mut overrides
                    = scc.iter()
                        .filter(|&&scc_locator| new_children.get(&scc_locator.ident).map_or(false, |&existing_node| &self.work_tree.nodes[existing_node].locator != scc_locator))
                        .peekable();

                if overrides.peek().is_some() {
                    if self.print_logs {
                        self.print(&format!(
                            "Cannot hoist {} into {} because the latter already hosts children with the same name",
                            scc.iter().map(|locator| locator.to_print_string()).join(", "),
                            node.locator.to_print_string(),
                        ));

                        while let Some(scc_locator) = overrides.next() {
                            let overridden_locator
                                = new_children.get(&scc_locator.ident)
                                    .map(|&idx| &self.work_tree.nodes[idx].locator)
                                    .expect("Expected the child to be present since otherwise there would be no override");

                            self.print(&format!(
                                "  - {} required {} but parent requires {} instead",
                                node.locator.to_print_string(), overridden_locator.to_print_string(), scc_locator.to_print_string(),
                            ));
                        }
                    }

                    continue;
                }

                // Now is the time to check if the requirements of the SCC are fulfilled by the
                // parent node. If they are not we can't hoist the SCC.

                let invalid_requirements = scc_hoisting_requirements.iter()
                    .flat_map(|(package, package_dependencies)| {
                        package_dependencies.iter().filter_map(|&dependency| {
                            // If the dependency is in the hoisting set it means it'll be hoisted atomatically with the
                            // current package, so it's all good.
                            if scc.contains(&dependency) {
                                return None;
                            }

                            let Some(current_resolution) = new_dependencies.get(&dependency.ident) else {
                                return Some((*package, dependency, None));
                            };

                            if current_resolution != dependency {
                                return Some((*package, dependency, Some(current_resolution)));
                            }

                            None
                        })
                    }).collect_vec();

                if !invalid_requirements.is_empty() {
                    if self.print_logs {
                        self.print(&format!(
                            "Cannot hoist {} into {} because some requirements are not fulfilled:",
                            scc.iter().map(|locator| locator.to_print_string()).join(", "),
                            node.locator.to_print_string(),
                        ));

                        for (package, dependency, current_resolution) in invalid_requirements {
                            self.print(&format!(
                                "  - {} required {} but parent requires {} instead",
                                package.to_print_string(),
                                dependency.to_print_string(),
                                current_resolution.map(|locator| locator.to_print_string()).unwrap_or_else(|| "<none>".to_string()),
                            ));
                        }
                    }

                    continue;
                }

                if self.print_logs {
                    self.print(&format!(
                        "Successfully hoisted {} into {}",
                        scc.iter().map(|locator| locator.to_print_string()).join(", "),
                        node.locator.to_print_string(),
                    ));
                }

                self.has_changed = true;

                for &hoisting_locator in scc {
                    // We need to hoist a node, but we only have the locator ... no big deal, we'll just
                    // steal the node from one of the old parents! They should all be valid anyway (and
                    // perhaps even identical although I'm not certain about that just yet).
                    let (_old_parent_idx, _child_ident, hoisting_idx)
                        = &selected_hoist_candidates[&hoisting_locator][0];

                    new_dependencies.insert(
                        hoisting_locator.ident.clone(),
                        hoisting_locator.clone(),
                    );

                    new_children.insert(
                        hoisting_locator.ident.clone(),
                        *hoisting_idx,
                    );

                    hoisted_dependencies_to_remove.push(hoisting_locator.clone());
                }
            }

            for hoisted_dependency_to_remove in hoisted_dependencies_to_remove {
                for (parent_idx, child_ident, _child_node_idx) in &selected_hoist_candidates[&hoisted_dependency_to_remove] {
                    self.work_tree.nodes[*parent_idx].children.as_mut().unwrap().remove(child_ident).unwrap();
                }
            }

            self.work_tree.nodes[node_idx].dependencies = new_dependencies;
            self.work_tree.nodes[node_idx].children = Some(new_children);
        }

        let node_locator
            = &self.work_tree.nodes[node_idx].locator;

        if let Some(self_child_idx) = self.work_tree.nodes[node_idx].children.as_ref().unwrap().get(&node_locator.ident) {
            let self_child_locator
                = self.work_tree.nodes[*self_child_idx].locator
                    .clone();

            if &self_child_locator == node_locator {
                self.work_tree.nodes[node_idx].children.as_mut().unwrap().remove(&self_child_locator.ident).unwrap();
            }
        }

        for child_idx in node_children {
            self.process_node(child_idx);
        }

        //self.seen[node_idx] = false;
        self.stack.pop();
    }
}
