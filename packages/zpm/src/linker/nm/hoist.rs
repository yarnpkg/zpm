use std::collections::{BTreeMap, BTreeSet};

use itertools::Itertools;
use zpm_primitives::{Ident, Locator};
use zpm_utils::{tree, ToHumanString};

use crate::{
    algos,
    install::InstallState,
    project::Project,
};

#[derive(Debug, Clone)]
pub struct InputNode {
    pub dependencies: BTreeMap<Ident, Locator>,
}

#[derive(Debug, Clone)]
pub struct InputTree {
    pub nodes: BTreeMap<Locator, InputNode>,
    pub root: Locator,
}

impl InputTree {
    pub fn from_install_state(project: &Project, install_state: &InstallState) -> Self {
        let mut node_map
            = BTreeMap::new();

        for (locator, resolution) in &install_state.resolution_tree.locator_resolutions {
            let dependencies
                = resolution.dependencies.iter()
                    .map(|(ident, descriptor)| (ident.clone(), install_state.resolution_tree.descriptor_to_locator[descriptor].clone()))
                    .collect();

            node_map.insert(locator.clone(), InputNode {dependencies});
        }

        Self {nodes: node_map, root: project.root_workspace().locator().clone()}
    }

    #[cfg(test)]
    pub fn from_test_tree(spec: BTreeMap<Locator, Vec<Locator>>) -> Self {
        use zpm_primitives::testing::l;

        let mut node_map
            = BTreeMap::new();

        let all_locators
            = spec.iter()
                .flat_map(|(locator, dependency_vec)| dependency_vec.iter().chain(Some(locator)))
                .collect::<Vec<_>>();

        for locator in all_locators {
            let dependencies
                = spec.get(locator)
                    .map(|dependencies| dependencies.iter().map(|l| (l.ident.clone(), l.clone())).collect())
                    .unwrap_or_default();

            node_map.insert(locator.clone(), InputNode {dependencies});
        }

        Self {nodes: node_map, root: l("root")}
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct WorkNode {
    pub locator: Locator,
    pub dependencies: BTreeMap<Ident, Locator>,
    pub children: BTreeMap<Ident, usize>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct WorkTree {
    pub nodes: Vec<WorkNode>,
}

impl WorkTree {
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
        }
    }

    pub fn from_input_tree(input_tree: &InputTree) -> Self {
        let mut work_tree
            = Self::new();

        work_tree.import_input_tree(input_tree);
        work_tree
    }

    #[cfg(test)]
    fn accessible_nodes(&self) -> Vec<usize> {
        let mut accessible_nodes
            = vec![];

        let mut stack
            = vec![0];

        while let Some(node_idx) = stack.pop() {
            accessible_nodes.push(node_idx);

            let node
                = &self.nodes[node_idx];

            for child_locator in node.children.values().rev() {
                stack.push(*child_locator);
            }
        }

        accessible_nodes
    }

    #[cfg(test)]
    fn inspect(&self, locators_to_names: &BTreeMap<Locator, &'static str>) -> BTreeMap<String, BTreeSet<String>> {
        let mut result: BTreeMap<_, BTreeSet<_>>
            = BTreeMap::new();

        let accessible_nodes
            = self.accessible_nodes();

        let duplicate_names
            = accessible_nodes.iter()
                .map(|node_idx| locators_to_names[&self.nodes[*node_idx].locator])
                .duplicates()
                .collect::<BTreeSet<_>>();

        let mut locator_ids
            = BTreeMap::new();

        let mut id_of = |node_idx: usize| {
            let locator
                = &self.nodes[node_idx].locator;

            let counter = locator_ids.entry(locator.clone())
                .or_insert(0);

            *counter += 1;
            *counter
        };

        let mut get_node_name = |node_idx: usize| {
            let base_name
                = locators_to_names[&self.nodes[node_idx].locator];

            if duplicate_names.contains(base_name) {
                format!("{}#{}", base_name, id_of(node_idx))
            } else {
                base_name.to_string()
            }
        };

        let nodes_to_names
            = accessible_nodes.iter()
                .map(|&node_idx| (node_idx, get_node_name(node_idx)))
                .collect::<BTreeMap<_, _>>();

        for node_idx in accessible_nodes {
            let node
                = &self.nodes[node_idx];

            let node_name
                = &nodes_to_names[&node_idx];

            for &child_idx in node.children.values() {
                let child_name
                    = &nodes_to_names[&child_idx];

                result.entry(node_name.clone()).or_default().insert(child_name.clone());
            }
        }

        result
    }

    fn import_input_tree(&mut self, input_tree: &InputTree) {
        let mut locator_to_idx
            = BTreeMap::new();

        let root_work_idx
            = self.import_input_node(input_tree, &input_tree.root);

        locator_to_idx.insert(input_tree.root.clone(), root_work_idx);

        for child_locator in input_tree.nodes[&input_tree.root].dependencies.values() {
            self.import_dfs(input_tree, child_locator, root_work_idx, &mut locator_to_idx);
        }
    }

    fn import_input_node(&mut self, input_tree: &InputTree, node_locator: &Locator) -> usize {
        let new_work_idx
            = self.nodes.len();

        self.nodes.push(WorkNode {
            locator: node_locator.clone(),
            dependencies: input_tree.nodes[node_locator].dependencies.clone(),
            children: BTreeMap::new(),
        });

        new_work_idx
    }

    fn import_dfs(&mut self, input_tree: &InputTree, node_locator: &Locator, parent_work_idx: usize, locator_to_idx: &mut BTreeMap<Locator, usize>) {
        let existing_work_idx
            = locator_to_idx.get(node_locator);

        if let Some(existing_work_idx) = existing_work_idx {
            // We remove self-dependencies; they don't really make sense, as
            // all packages have an implicit dependency on themselves anwyay.
            if *existing_work_idx == parent_work_idx {
                return;
            }

            let parent_node
                = &mut self.nodes[parent_work_idx];

            parent_node.children.insert(
                node_locator.ident.clone(),
                *existing_work_idx,
            );
        } else {
            let node
                = &input_tree.nodes[node_locator];

            let new_work_idx
                = self.import_input_node(input_tree, node_locator);

            let parent_node
                = &mut self.nodes[parent_work_idx];

            parent_node.children.insert(
                node_locator.ident.clone(),
                new_work_idx,
            );

            locator_to_idx.insert(node_locator.clone(), new_work_idx);

            for child_locator in node.dependencies.values() {
                self.import_dfs(input_tree, child_locator, new_work_idx, locator_to_idx);
            }

            locator_to_idx.remove(node_locator);
        }
    }
}

pub struct TreeRenderer<'a> {
    tree: &'a WorkTree,
    parent_stack: Vec<usize>,
}

impl<'a> TreeRenderer<'a> {
    pub fn new(tree: &'a WorkTree) -> Self {
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

        let invalid_dependencies
            = node.dependencies.values()
                .filter(|locator| available_dependencies.get(&locator.ident) != Some(locator))
                .collect::<Vec<_>>();

        let mut label
            = node.locator.to_print_string();

        if is_cycle {
            label.push_str(" (cycle)");
        }

        let mut children
            = Vec::new();

        for &locator in &invalid_dependencies {
            let invalid_dependency_locator
                = available_dependencies.get(&locator.ident);

            let label = format!(
                "❌ Invalid dependency {} (expected {})",
                invalid_dependency_locator.map(|locator| locator.to_print_string()).unwrap_or_else(|| "<none>".to_string()),
                locator.to_print_string(),
            );

            children.push(tree::Node {
                label: Some(label),
                value: None,
                children: None,
            });
        }

        for &child_idx in self.tree.nodes[node_idx].children.values() {
            children.push(self.convert_impl(child_idx, available_dependencies.clone()));
        }

        self.parent_stack.pop();

        tree::Node {
            label: Some(label),
            value: None,
            children: Some(tree::TreeNodeChildren::Vec(children)),
        }
    }

    fn extend_available_dependencies(&self, mut available_dependencies: BTreeMap<&'a Ident, &'a Locator>, node_idx: usize) -> BTreeMap<&'a Ident, &'a Locator> {
        let node
            = &self.tree.nodes[node_idx];

        available_dependencies.insert(&node.locator.ident, &node.locator);

        available_dependencies.extend(node.children.iter().map(|(ident, child_idx)| {
            let child_node
                = &self.tree.nodes[*child_idx];

            (ident, &child_node.locator)
        }));

        available_dependencies
    }
}

pub struct Hoister<'a> {
    work_tree: &'a mut WorkTree,
    seen: Vec<bool>,
    stack: Vec<usize>,
    has_changed: bool,
    print_logs: bool,
}

impl<'a> Hoister<'a> {
    pub fn new(work_tree: &'a mut WorkTree) -> Self {
        Self {
            work_tree,
            seen: vec![],
            stack: vec![],
            has_changed: false,
            print_logs: false,
        }
    }

    pub fn set_print_logs(&mut self, print_logs: bool) {
        self.print_logs = print_logs;
    }

    pub fn hoist(&mut self) {
        self.seen = vec![false; self.work_tree.nodes.len()];
        self.stack.clear();

        self.has_changed = true;

        while self.has_changed {
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

        self.seen[node_idx] = true;
        self.stack.push(node_idx);

        // We need to clone it to please the borrow checker.
        let node_children
            = &self.work_tree.nodes[node_idx]
                .children.clone();

        let mut hoist_candidates_with_parents: BTreeMap<Locator, Vec<usize>>
            = BTreeMap::new();

        for &child_idx in node_children.values() {
            if self.seen[child_idx] {
                continue;
            }

            self.process_node(child_idx);

            let flattened_node
                = &self.work_tree.nodes[child_idx];

            let transitive_children
                = flattened_node.children.clone();

            for &transitive_node in transitive_children.values() {
                let parents = hoist_candidates_with_parents
                    .entry(self.work_tree.nodes[transitive_node].locator.clone())
                        .or_default();

                parents.push(child_idx);
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
                        = self.work_tree.nodes[node_idx].children.get(&transitive_locator.ident);

                    let existing_child_locator = existing_child_idx
                        .map(|&idx| &self.work_tree.nodes[idx].locator);

                    existing_child_locator == Some(transitive_locator)
                });

        for (transitive_locator, parents) in locators_to_meld {
            for parent_idx in parents {
                self.work_tree.nodes[parent_idx].children.remove(&transitive_locator.ident).unwrap();
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
                let parent_node
                    = &self.work_tree.nodes[parents[0]];

                let hoistable_idx
                    = parent_node
                        .children[&hoistable_locator.ident];

                let hoistable_node
                    = &self.work_tree.nodes[hoistable_idx];

                let mut candidate_dependencies
                    = BTreeSet::new();

                for dependency_locator in hoistable_node.dependencies.values() {
                    // This node already embed its own copy of its dependency, so we don't care about whatever
                    // else could conflict with the parent.
                    if hoistable_node.children.contains_key(&dependency_locator.ident) {
                        continue;
                    }

                    // The parent doesn't contain the dependency, which means hoisting the hoistable candidate
                    // into our package wouldn't break the dependency any more than it already is.
                    if !parent_node.children.contains_key(&dependency_locator.ident) {
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
                = self.work_tree.nodes[node_idx].children.clone();

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

                let coherent_parent
                    = scc.iter().all(|package| {
                        package.ident != node.locator.ident || *package == &node.locator
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

                let no_dependency_conflicts
                    = scc.iter().all(|package| {
                        let start_dependency
                            = node.dependencies.get(&package.ident);

                        start_dependency.map_or(true, |existing_locator| {
                            &existing_locator == package
                        })
                    });

                if !no_dependency_conflicts {
                    if self.print_logs {
                        self.print(&format!(
                            "Cannot hoist {} into {} because the former conflicts with the dependencies of the latter",
                            scc.iter().map(|locator| locator.to_print_string()).join(", "),
                            node.locator.to_print_string(),
                        ));
                    }

                    continue;
                }

                let no_overrides
                    = scc.iter().all(|&package| {
                        new_children.get(&package.ident).map_or(true, |&existing_node| {
                            &self.work_tree.nodes[existing_node].locator == package
                        })
                    });

                if !no_overrides {
                    if self.print_logs {
                        self.print(&format!(
                            "Cannot hoist {} into {} because the latter already hosts children with the same name",
                            scc.iter().map(|locator| locator.to_print_string()).join(", "),
                            node.locator.to_print_string(),
                        ));
                    }

                    continue;
                }

                let invalid_requirements = scc_hoisting_requirements.iter()
                    .flat_map(|(package, package_dependencies)| {
                        package_dependencies.iter().filter_map(|dependency| {
                            // If the dependency is in the hoisting set it means it'll be hoisted atomatically with the
                            // current package, so it's all good.
                            if scc.contains(&dependency) {
                                return None;
                            }

                            let current_resolution
                                = new_dependencies.get(&dependency.ident);

                            let is_existing_requirement = current_resolution.map_or(false, |existing_requirement| {
                                &existing_requirement == dependency
                            });

                            if is_existing_requirement {
                                return None;
                            }

                            Some((*package, *dependency, current_resolution))
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
                    let old_parent_idx
                        = selected_hoist_candidates[&hoisting_locator][0];

                    let old_parent_node
                        = &self.work_tree.nodes[old_parent_idx];

                    let hoisting_idx
                        = old_parent_node.children[&hoisting_locator.ident];

                    new_dependencies.insert(
                        hoisting_locator.ident.clone(),
                        hoisting_locator.clone(),
                    );

                    new_children.insert(
                        hoisting_locator.ident.clone(),
                        hoisting_idx,
                    );

                    hoisted_dependencies_to_remove.push(hoisting_locator.clone());
                }
            }

            for hoisted_dependency_to_remove in hoisted_dependencies_to_remove {
                for parent_locator in &selected_hoist_candidates[&hoisted_dependency_to_remove] {
                    self.work_tree.nodes[*parent_locator].children.remove(&hoisted_dependency_to_remove.ident);
                }
            }

            self.work_tree.nodes[node_idx].dependencies = new_dependencies;
            self.work_tree.nodes[node_idx].children = new_children;
        }

        let node_locator
            = &self.work_tree.nodes[node_idx].locator;

        if let Some(self_child_idx) = self.work_tree.nodes[node_idx].children.get(&node_locator.ident) {
            let self_child_locator
                = self.work_tree.nodes[*self_child_idx].locator
                    .clone();

            if &self_child_locator == node_locator {
                self.work_tree.nodes[node_idx].children.remove(&self_child_locator.ident).unwrap();
            }
        }

        self.seen[node_idx] = false;
        self.stack.pop();
    }
}

#[cfg(test)]
mod tests {
    use zpm_primitives::{testing::{i, l}, dependency_map};

    use super::*;

    #[test]
    fn it_should_import_a_simple_tree_with_no_duplicates() {
        let input_tree = InputTree::from_test_tree(BTreeMap::from_iter([
            (l("root"), vec![l("a"), l("b")]),
            (l("a"), vec![l("d")]),
            (l("b"), vec![l("e")]),
        ]));

        let work_tree
            = WorkTree::from_input_tree(&input_tree);

        assert_eq!(work_tree, WorkTree {
            nodes: vec![
                WorkNode {
                    locator: l("root"),
                    dependencies: dependency_map![
                        l("a"),
                        l("b"),
                    ],
                    children: BTreeMap::from_iter([
                        (i("a"), 1),
                        (i("b"), 3),
                    ]),
                },
                WorkNode {
                    locator: l("a"),
                    dependencies: dependency_map![
                        l("d"),
                    ],
                    children: BTreeMap::from_iter([
                        (i("d"), 2),
                    ]),
                },
                WorkNode {
                    locator: l("d"),
                    dependencies: BTreeMap::new(),
                    children: BTreeMap::new(),
                },
                WorkNode {
                    locator: l("b"),
                    dependencies: dependency_map![
                        l("e"),
                    ],
                    children: BTreeMap::from_iter([
                        (i("e"), 4),
                    ]),
                },
                WorkNode {
                    locator: l("e"),
                    dependencies: BTreeMap::new(),
                    children: BTreeMap::new(),
                },
            ],
        });
    }

    #[test]
    fn it_should_unfold_a_simple_tree_with_some_duplicates() {
        let input_tree = InputTree::from_test_tree(BTreeMap::from_iter([
            (l("root"), vec![l("a"), l("b")]),
            (l("a"), vec![l("c")]),
            (l("b"), vec![l("c")]),
        ]));

        let work_tree
            = WorkTree::from_input_tree(&input_tree);

        assert_eq!(work_tree, WorkTree {
            nodes: vec![
                WorkNode {
                    locator: l("root"),
                    dependencies: dependency_map![
                        l("a"),
                        l("b"),
                    ],
                    children: BTreeMap::from_iter([
                        (i("a"), 1),
                        (i("b"), 3),
                    ]),
                },
                WorkNode {
                    locator: l("a"),
                    dependencies: dependency_map![
                        l("c"),
                    ],
                    children: BTreeMap::from_iter([
                        (i("c"), 2),
                    ]),
                },
                WorkNode {
                    locator: l("c"),
                    dependencies: BTreeMap::new(),
                    children: BTreeMap::new(),
                },
                WorkNode {
                    locator: l("b"),
                    dependencies: dependency_map![
                        l("c"),
                    ],
                    children: BTreeMap::from_iter([
                        (i("c"), 4),
                    ]),
                },
                WorkNode {
                    locator: l("c"),
                    dependencies: BTreeMap::new(),
                    children: BTreeMap::new(),
                },
            ],
        });
    }

    fn test_hoisting(spec: BTreeMap<&'static str, Vec<&'static str>>, expected_hoisting: BTreeMap<&'static str, Vec<&'static str>>) {
        let all_names
            = spec.iter()
                .flat_map(|(k, v)| v.iter().chain(Some(k)))
                .cloned()
                .collect::<Vec<_>>();

        let name_to_locators
            = all_names.iter()
                .map(|&name| (name, l(name)))
                .collect::<BTreeMap<_, _>>();

        let locator_to_name
            = name_to_locators.iter()
                .map(|(&k, v)| (v.clone(), k))
                .collect::<BTreeMap<_, _>>();

        let input_tree
            = InputTree::from_test_tree(spec.into_iter()
                .map(|(k, v)| (name_to_locators[k].clone(), v.into_iter().map(|name| name_to_locators[name].clone()).collect()))
                .collect());

        let mut work_tree
            = WorkTree::from_input_tree(&input_tree);

        let mut hoister
            = Hoister::new(&mut work_tree);

        hoister.set_print_logs(true);
        hoister.hoist();

        let expected_hoisting
            = expected_hoisting.into_iter()
                .map(|(k, v)| (k.to_string(), v.into_iter().map(|s| s.to_string()).collect())).collect();

        assert_eq!(work_tree.inspect(&locator_to_name), expected_hoisting);
    }

    #[test]
    fn it_should_hoist_a_basic_tree() {
        test_hoisting(
            BTreeMap::from_iter([
                ("root", vec!["a", "b"]),
                ("a", vec!["c"]),
                ("b", vec!["c"]),
            ]),
            BTreeMap::from_iter([
                ("root", vec!["a", "b", "c"]),
            ]),
        );
    }

    #[test]
    fn it_should_hoist_a_tree_with_a_simple_loop() {
        test_hoisting(
            BTreeMap::from_iter([
                ("root", vec!["a"]),
                ("a", vec!["b"]),
                ("b", vec!["a"]),
            ]),
            BTreeMap::from_iter([
                ("root", vec!["a", "b"]),
            ]),
        );
    }

    #[test]
    fn it_should_hoist_a_tree_with_a_deep_loop() {
        test_hoisting(
            BTreeMap::from_iter([
                ("root", vec!["c"]),
                ("a", vec!["b", "d"]),
                ("b", vec!["a"]),
                ("c", vec!["a"]),
                ("d", vec!["e"]),
            ]),
            BTreeMap::from_iter([
                ("root", vec!["a", "b", "c", "d", "e"]),
            ]),
        );
    }

    #[test]
    fn it_should_select_the_most_popular_version_of_a_package() {
        test_hoisting(
            BTreeMap::from_iter([
                ("root", vec!["a", "b", "c"]),
                ("a", vec!["d@2"]),
                ("b", vec!["d@1"]),
                ("c", vec!["d@2"]),
            ]),
            BTreeMap::from_iter([
                ("root", vec!["a", "b", "c", "d@2"]),
                ("b", vec!["d@1"]),
            ]),
        );
    }


    #[test]
    fn it_should_select_the_most_popular_version_of_a_package_reverse() {
        test_hoisting(
            BTreeMap::from_iter([
                ("root", vec!["a", "b", "c"]),
                ("a", vec!["d@1"]),
                ("b", vec!["d@2"]),
                ("c", vec!["d@1"]),
            ]),
            BTreeMap::from_iter([
                ("root", vec!["a", "b", "c", "d@1"]),
                ("b", vec!["d@2"]),
            ]),
        );
    }

    #[test]
    fn it_shouldnt_hoist_a_package_if_it_would_break_a_self_dependency() {
        test_hoisting(
            BTreeMap::from_iter([
                ("root", vec!["a@1", "b@1"]),
                ("a@1", vec!["b@2"]),
                ("b@2", vec!["a@2"]),
            ]),
            BTreeMap::from_iter([
                ("root", vec!["a@1", "b@1"]),
                ("a@1", vec!["b@2"]),
                ("b@2", vec!["a@2"]),
            ]),
        );
    }

    #[test]
    fn it_shouldnt_hoist_a_package_if_its_dependencies_cannot_be_hoisted_either() {
        test_hoisting(
            BTreeMap::from_iter([
                ("root", vec!["a", "c@1"]),
                ("a", vec!["b"]),
                ("b", vec!["c@2"]),
            ]),
            BTreeMap::from_iter([
                ("root", vec!["a", "c@1"]),
                ("a", vec!["b", "c@2"]),
            ]),
        );
    }

    #[test]
    fn it_should_hoist_different_copies_of_a_package_independently() {
        test_hoisting(
            BTreeMap::from_iter([
                ("root", vec!["a", "b@2", "c@3", "d"]),
                ("a", vec!["b@1", "c@2"]),
                ("b@1", vec!["c@1"]),
                ("d", vec!["b@1"]),
            ]),
            BTreeMap::from_iter([
                ("root", vec!["a", "b@2", "c@3", "d"]),
                ("a", vec!["b@1#1", "c@2"]),
                ("b@1#1", vec!["c@1#1"]),
                ("d", vec!["b@1#2", "c@1#2"]),
            ]),
        );
    }

    #[test]
    fn it_should_hoist_different_copies_of_a_package_independently_complex() {
        // const tree = {
        //     '.': {dependencies: [`A`, `E`, `F`, `B@Y`, `C@Z`, `D@Y`]},
        //     A: {dependencies: [`B@X`, `C@Y`]},
        //     'B@X': {dependencies: [`C@X`]},
        //     'C@X': {dependencies: [`D@X`]},
        //     E: {dependencies: [`B@X`]},
        //     F: {dependencies: [`G`]},
        //     G: {dependencies: [`B@X`, `D@Z`]},
        //   };
        test_hoisting(
            BTreeMap::from_iter([
                ("root", vec!["a", "e", "f", "b@2", "c@3", "d@2"]),
                ("a", vec!["b@1", "c@2"]),
                ("b@1", vec!["c@1"]),
                ("c@1", vec!["d@1"]),
                ("e", vec!["b@1"]),
                ("f", vec!["g"]),
                ("g", vec!["b@1", "d@3"]),
            ]),
            BTreeMap::from_iter([
                ("root", vec!["a", "e", "f", "b@2", "c@3", "d@2"]),
                ("a", vec!["b@1#1", "c@2", "d@1#1"]),
                ("b@1#1", vec!["c@1#1"]),
                ("b@1#3", vec!["c@1#3", "d@1#3"]),
                ("e", vec!["b@1#2", "c@1#2", "d@1#2"]),
                ("f", vec!["b@1#3", "d@3", "g"]),
            ]),
        );
    }

    #[test]
    fn it_shouldnt_hoist_a_package_that_another_package_higher_up_in_the_tree_depends_on() {
        test_hoisting(
            BTreeMap::from_iter([
                ("root", vec!["a", "b@2", "c@2", "d@2"]),
                ("a", vec!["b@1", "c@1", "d@1"]),
                ("b@1", vec!["e@2"]),
                ("c@1", vec!["e@1"]),
                ("d@1", vec!["e@1"]),
            ]),
            BTreeMap::from_iter([
                ("root", vec!["a", "b@2", "c@2", "d@2", "e@1"]),
                ("a", vec!["b@1", "c@1", "d@1"]),
                ("b@1", vec!["e@2"]),
            ]),
        );
    }

    // . -> @eslint/eslintrc@3
    //   -> @typescript-eslint/parser -> @typescript-eslint/typescript-estree -> minimatch@2
    //                                -> eslint -> @eslint/eslintrc -> minimatch@1
    //   -> @typescript-eslint/typescript-estree@3
    //   -> eslint@3
    //   -> minimatch@1

    // a <=> @eslint/eslintrc
    // b <=> @typescript-eslint/parser
    // c <=> @typescript-eslint/typescript-estree
    // d <=> eslint
    // e <=> minimatch

    #[test]
    fn it_should_toto() {
        test_hoisting(
            BTreeMap::from_iter([
                ("root", vec!["a@3", "b", "c@3", "d@3", "e@1"]),
                ("a@1", vec!["e@1"]),
                ("b", vec!["c@1", "d@1"]),
                ("c@1", vec!["e@2"]),
                ("d@1", vec!["a@1"]),
            ]),
            BTreeMap::from_iter([
                ("root", vec!["a@3", "b", "c@3", "d@3", "e@1#2"]),
                ("b", vec!["c@1", "d@1", "e@2"]),
                ("d@1", vec!["a@1", "e@1#1"]),
            ]),
        );
    }
}
