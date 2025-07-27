#[cfg(test)]
mod tests {
    use super::super::hoist::*;
    use indexmap::{IndexMap, set::IndexSet};

    // Helper struct to build test trees more easily
    #[derive(Clone)]
    struct TestNode {
        dependencies: Vec<String>,
        peer_names: Vec<String>,
        ident_name: Option<String>,
        dependency_kind: Option<HoisterDependencyKind>,
    }

    impl Default for TestNode {
        fn default() -> Self {
            TestNode {
                dependencies: Vec::new(),
                peer_names: Vec::new(),
                ident_name: None,
                dependency_kind: None,
            }
        }
    }

    // Convert a IndexMap representation to a HoisterTree
    fn to_tree(obj: IndexMap<&str, TestNode>) -> HoisterTree {
        let mut tree_nodes = Vec::new();
        let mut key_to_id = IndexMap::new();

        // First pass: create all nodes
        let mut sorted_keys: Vec<_> = obj.keys().cloned().collect();
        sorted_keys.sort(); // Ensure consistent ordering

        // Make sure root node "." is first
        if let Some(pos) = sorted_keys.iter().position(|&k| k == ".") {
            sorted_keys.remove(pos);
            sorted_keys.insert(0, ".");
        }

        for key in &sorted_keys {
            let (name, reference) = if let Some(at_pos) = key.find('@') {
                (&key[..at_pos], &key[at_pos + 1..])
            } else {
                (*key, "")
            };

            let id = tree_nodes.len();
            key_to_id.insert(*key, id);

            let node_info = obj.get(key).cloned().unwrap_or_default();

            tree_nodes.push(HoisterNode {
                id,
                name: name.to_string(),
                ident_name: node_info.ident_name.unwrap_or_else(|| name.to_string()),
                reference: reference.to_string(),
                dependencies: IndexSet::new(),
                peer_names: node_info.peer_names.into_iter().collect(),
                hoist_priority: None,
                dependency_kind: if id == 0 {
                    Some(HoisterDependencyKind::Workspace)
                } else {
                    node_info.dependency_kind
                },
            });
        }

        // Second pass: add dependencies
        for (key, node_info) in &obj {
            let id = key_to_id[key];

            for dep in &node_info.dependencies {
                if let Some(&dep_id) = key_to_id.get(dep.as_str()) {
                    tree_nodes[id].dependencies.insert(dep_id);
                }
            }
        }

        HoisterTree {
            nodes: tree_nodes,
            root: 0,
        }
    }

    // Calculate the height of the hoisted tree
    fn get_tree_height(tree: &HoisterResult) -> usize {
        fn visit_node(node: &HoisterResult, seen: &mut IndexSet<String>, depth: usize) -> usize {
            let key = format!("{}@{}", node.name, node.ident_name);
            if seen.contains(&key) {
                return depth;
            }
            seen.insert(key);

            let mut max_depth = depth;
            for dep in &node.dependencies {
                max_depth = max_depth.max(visit_node(dep, seen, depth + 1));
            }
            max_depth
        }

        let mut seen = IndexSet::new();
        visit_node(tree, &mut seen, 1)
    }

    #[test]
    fn should_do_very_basic_hoisting() {
        // . -> A -> B
        // should be hoisted to:
        // . -> A
        //   -> B
        let mut tree = IndexMap::new();
        tree.insert(".", TestNode {
            dependencies: vec!["A".to_string()],
            ..Default::default()
        });
        tree.insert("A", TestNode {
            dependencies: vec!["B".to_string()],
            ..Default::default()
        });
        tree.insert("B", TestNode::default());

        let result = hoist(to_tree(tree), Some(HoistOptions {
            check: Some(true),
            ..Default::default()
        })).unwrap();

        assert_eq!(get_tree_height(&result), 2);
    }

    #[test]
    fn should_support_basic_cyclic_dependencies() {
        // . -> C -> A -> B -> A
        //             -> D -> E
        // should be hoisted to:
        // . -> A
        //   -> B
        //   -> C
        //   -> D
        //   -> E
        let mut tree = IndexMap::new();
        tree.insert(".", TestNode {
            dependencies: vec!["C".to_string()],
            ..Default::default()
        });
        tree.insert("C", TestNode {
            dependencies: vec!["A".to_string()],
            ..Default::default()
        });
        tree.insert("A", TestNode {
            dependencies: vec!["B".to_string(), "D".to_string()],
            ..Default::default()
        });
        tree.insert("B", TestNode {
            dependencies: vec!["A".to_string(), "E".to_string()],
            ..Default::default()
        });
        tree.insert("D", TestNode::default());
        tree.insert("E", TestNode::default());

        let result = hoist(to_tree(tree), Some(HoistOptions {
            check: Some(true),
            ..Default::default()
        })).unwrap();

        assert_eq!(get_tree_height(&result), 2);
    }

    #[test]
    fn should_support_cyclic_peer_dependencies() {
        // . -> E@X
        //   -> D -> A --> B
        //        -> B --> C
        //        -> C --> A
        //             --> E@Y
        //        -> E@Y
        // Should be hoisted to:
        // . -> E@X
        //   -> D -> A
        //        -> B
        //        -> C
        //        -> E@Y
        let mut tree = IndexMap::new();
        tree.insert(".", TestNode {
            dependencies: vec!["D".to_string(), "E@X".to_string()],
            ..Default::default()
        });
        tree.insert("D", TestNode {
            dependencies: vec!["A".to_string(), "B".to_string(), "C".to_string(), "E@Y".to_string()],
            ..Default::default()
        });
        tree.insert("A", TestNode {
            dependencies: vec!["B".to_string()],
            peer_names: vec!["B".to_string()],
            ..Default::default()
        });
        tree.insert("B", TestNode {
            dependencies: vec!["C".to_string()],
            peer_names: vec!["C".to_string()],
            ..Default::default()
        });
        tree.insert("C", TestNode {
            dependencies: vec!["A".to_string(), "E@Y".to_string()],
            peer_names: vec!["A".to_string(), "E".to_string()],
            ..Default::default()
        });
        tree.insert("E@X", TestNode::default());
        tree.insert("E@Y", TestNode::default());

        let result = hoist(to_tree(tree), Some(HoistOptions {
            check: Some(true),
            ..Default::default()
        })).unwrap();

        assert_eq!(get_tree_height(&result), 3);
    }

    #[test]
    fn should_keep_require_promise() {
        // . -> A -> B -> C@X -> D@X
        //             -> F@X -> G@X
        //        -> C@Z
        //        -> F@Z
        //   -> C@Y
        //   -> D@Y
        // should be hoisted to:
        // . -> A
        //        -> C@Z
        //        -> D@X
        //   -> B -> C@X
        //        -> F@X
        //   -> C@Y
        //   -> D@Y
        //   -> F@Z
        //   -> G@X
        let mut tree = IndexMap::new();
        tree.insert(".", TestNode {
            dependencies: vec!["A".to_string(), "C@Y".to_string(), "D@Y".to_string()],
            ..Default::default()
        });
        tree.insert("A", TestNode {
            dependencies: vec!["B".to_string(), "C@Z".to_string(), "F@Z".to_string()],
            ..Default::default()
        });
        tree.insert("B", TestNode {
            dependencies: vec!["C@X".to_string(), "F@X".to_string()],
            ..Default::default()
        });
        tree.insert("F@X", TestNode {
            dependencies: vec!["G@X".to_string()],
            ..Default::default()
        });
        tree.insert("C@X", TestNode {
            dependencies: vec!["D@X".to_string()],
            ..Default::default()
        });
        tree.insert("C@Y", TestNode::default());
        tree.insert("C@Z", TestNode::default());
        tree.insert("D@X", TestNode::default());
        tree.insert("D@Y", TestNode::default());
        tree.insert("F@Z", TestNode::default());
        tree.insert("G@X", TestNode::default());

        let result = hoist(to_tree(tree), Some(HoistOptions {
            check: Some(true),
            ..Default::default()
        })).unwrap();

        assert_eq!(get_tree_height(&result), 3);
    }

    #[test]
    fn should_not_forget_hoisted_dependencies() {
        // . -> A -> B -> C@X
        //             -> A
        //   -> C@Y
        // should be hoisted to (B cannot be hoisted to the top, otherwise it will require C@Y instead of C@X)
        // . -> A -> B
        //        -> C@X
        //   -> C@Y
        let mut tree = IndexMap::new();
        tree.insert(".", TestNode {
            dependencies: vec!["A".to_string(), "C@Y".to_string()],
            ..Default::default()
        });
        tree.insert("A", TestNode {
            dependencies: vec!["B".to_string()],
            ..Default::default()
        });
        tree.insert("B", TestNode {
            dependencies: vec!["A".to_string(), "C@X".to_string()],
            ..Default::default()
        });
        tree.insert("C@X", TestNode::default());
        tree.insert("C@Y", TestNode::default());

        let result = hoist(to_tree(tree), Some(HoistOptions {
            check: Some(true),
            ..Default::default()
        })).unwrap();

        assert_eq!(get_tree_height(&result), 3);
    }

    #[test]
    fn should_not_hoist_different_package_with_same_name() {
        // . -> A -> B@X
        //   -> B@Y
        // should not be changed
        let mut tree = IndexMap::new();
        tree.insert(".", TestNode {
            dependencies: vec!["A".to_string(), "B@Y".to_string()],
            ..Default::default()
        });
        tree.insert("A", TestNode {
            dependencies: vec!["B@X".to_string()],
            ..Default::default()
        });
        tree.insert("B@X", TestNode::default());
        tree.insert("B@Y", TestNode::default());

        let result = hoist(to_tree(tree), Some(HoistOptions {
            check: Some(true),
            ..Default::default()
        })).unwrap();

        assert_eq!(get_tree_height(&result), 3);
    }

    #[test]
    fn should_not_hoist_package_with_several_versions_on_same_path() {
        // . -> A -> B@X -> C -> B@Y
        // should be hoisted to:
        // . -> A
        //   -> B@X
        //   -> C -> B@Y
        let mut tree = IndexMap::new();
        tree.insert(".", TestNode {
            dependencies: vec!["A".to_string()],
            ..Default::default()
        });
        tree.insert("A", TestNode {
            dependencies: vec!["B@X".to_string()],
            ..Default::default()
        });
        tree.insert("B@X", TestNode {
            dependencies: vec!["C".to_string()],
            ..Default::default()
        });
        tree.insert("C", TestNode {
            dependencies: vec!["B@Y".to_string()],
            ..Default::default()
        });
        tree.insert("B@Y", TestNode::default());

        let result = hoist(to_tree(tree), Some(HoistOptions {
            check: Some(true),
            ..Default::default()
        })).unwrap();

        assert_eq!(get_tree_height(&result), 3);
    }

    #[test]
    fn should_perform_deep_hoisting() {
        // . -> A -> B@X -> C@Y
        //        -> C@X
        //   -> B@Y
        //   -> C@X
        // should be hoisted to:
        // . -> A -> B@X -> C@Y
        //   -> B@Y
        //   -> C@X
        let mut tree = IndexMap::new();
        tree.insert(".", TestNode {
            dependencies: vec!["A".to_string(), "B@Y".to_string(), "C@X".to_string()],
            ..Default::default()
        });
        tree.insert("A", TestNode {
            dependencies: vec!["B@X".to_string(), "C@X".to_string()],
            ..Default::default()
        });
        tree.insert("B@X", TestNode {
            dependencies: vec!["C@Y".to_string()],
            ..Default::default()
        });
        tree.insert("B@Y", TestNode::default());
        tree.insert("C@X", TestNode::default());
        tree.insert("C@Y", TestNode::default());

        let result = hoist(to_tree(tree), Some(HoistOptions {
            check: Some(true),
            ..Default::default()
        })).unwrap();

        assert_eq!(get_tree_height(&result), 4);
    }

    #[test]
    fn should_tolerate_self_dependencies() {
        // . -> . -> A -> A -> B@X -> B@X -> C@Y
        //                  -> C@X
        //   -> B@Y
        //   -> C@X
        // should be hoisted to:
        // . -> A -> B@X -> C@Y
        //   -> B@Y
        //   -> C@X
        let mut tree = IndexMap::new();
        tree.insert(".", TestNode {
            dependencies: vec![".".to_string(), "A".to_string(), "B@Y".to_string(), "C@X".to_string()],
            ..Default::default()
        });
        tree.insert("A", TestNode {
            dependencies: vec!["A".to_string(), "B@X".to_string(), "C@X".to_string()],
            ..Default::default()
        });
        tree.insert("B@X", TestNode {
            dependencies: vec!["B@X".to_string(), "C@Y".to_string()],
            ..Default::default()
        });
        tree.insert("B@Y", TestNode::default());
        tree.insert("C@X", TestNode::default());
        tree.insert("C@Y", TestNode::default());

        let result = hoist(to_tree(tree), Some(HoistOptions {
            check: Some(true),
            ..Default::default()
        })).unwrap();

        assert_eq!(get_tree_height(&result), 4);
    }

    #[test]
    fn should_honor_package_popularity() {
        // . -> A -> B@X
        //   -> C -> B@X
        //   -> D -> B@Y
        //   -> E -> B@Y
        //   -> F -> G -> B@Y
        // should be hoisted to:
        // . -> A -> B@X
        //   -> C -> B@X
        //   -> D
        //   -> E
        //   -> F
        //   -> G
        //   -> B@Y
        let mut tree = IndexMap::new();
        tree.insert(".", TestNode {
            dependencies: vec!["A".to_string(), "C".to_string(), "D".to_string(), "E".to_string(), "F".to_string()],
            ..Default::default()
        });
        tree.insert("A", TestNode {
            dependencies: vec!["B@X".to_string()],
            ..Default::default()
        });
        tree.insert("C", TestNode {
            dependencies: vec!["B@X".to_string()],
            ..Default::default()
        });
        tree.insert("D", TestNode {
            dependencies: vec!["B@Y".to_string()],
            ..Default::default()
        });
        tree.insert("E", TestNode {
            dependencies: vec!["B@Y".to_string()],
            ..Default::default()
        });
        tree.insert("F", TestNode {
            dependencies: vec!["G".to_string()],
            ..Default::default()
        });
        tree.insert("G", TestNode {
            dependencies: vec!["B@Y".to_string()],
            ..Default::default()
        });
        tree.insert("B@X", TestNode::default());
        tree.insert("B@Y", TestNode::default());

        let result = hoist(to_tree(tree), Some(HoistOptions {
            check: Some(true),
            ..Default::default()
        })).unwrap();

        assert_eq!(get_tree_height(&result), 3);
    }

    #[test]
    fn should_honor_peer_dependencies() {
        // . -> A -> B --> D@X
        //        -> D@X
        //   -> D@Y
        // should be hoisted to (A and B should share single D@X dependency):
        // . -> A -> B
        //        -> D@X
        //   -> D@Y
        let mut tree = IndexMap::new();
        tree.insert(".", TestNode {
            dependencies: vec!["A".to_string(), "D@Y".to_string()],
            ..Default::default()
        });
        tree.insert("A", TestNode {
            dependencies: vec!["B".to_string(), "D@X".to_string()],
            ..Default::default()
        });
        tree.insert("B", TestNode {
            dependencies: vec!["D@X".to_string()],
            peer_names: vec!["D".to_string()],
            ..Default::default()
        });
        tree.insert("D@X", TestNode::default());
        tree.insert("D@Y", TestNode::default());

        let result = hoist(to_tree(tree), Some(HoistOptions {
            check: Some(true),
            ..Default::default()
        })).unwrap();

        assert_eq!(get_tree_height(&result), 3);
    }

    #[test]
    fn should_honor_package_popularity_with_peer_refs() {
        // . -> A -> Z@X
        //   -> B -> Z@X
        //   -> C -> Z@X
        //   -> D -> Z@Y
        //        -> U -> Z@Y
        // should be hoisted to:
        // . -> A
        //   -> B
        //   -> C
        //   -> D -> U
        //        -> Z@Y
        //   -> Z@X
        let mut tree = IndexMap::new();
        tree.insert(".", TestNode {
            dependencies: vec!["A".to_string(), "B".to_string(), "C".to_string(), "D".to_string()],
            ..Default::default()
        });
        tree.insert("A", TestNode {
            dependencies: vec!["Z@X".to_string()],
            ..Default::default()
        });
        tree.insert("B", TestNode {
            dependencies: vec!["Z@X".to_string()],
            ..Default::default()
        });
        tree.insert("C", TestNode {
            dependencies: vec!["Z@X".to_string()],
            ..Default::default()
        });
        tree.insert("D", TestNode {
            dependencies: vec!["Z@Y".to_string(), "U".to_string()],
            ..Default::default()
        });
        tree.insert("U", TestNode {
            dependencies: vec!["Z@Y".to_string()],
            peer_names: vec!["Z".to_string()],
            ..Default::default()
        });
        tree.insert("Z@X", TestNode::default());
        tree.insert("Z@Y", TestNode::default());

        let result = hoist(to_tree(tree), Some(HoistOptions {
            check: Some(true),
            ..Default::default()
        })).unwrap();

        assert_eq!(get_tree_height(&result), 3);

        // Check that Z@X is hoisted to top level
        let hoisted_z = result.dependencies.iter().find(|d| d.name == "Z").unwrap();
        assert!(hoisted_z.references.contains("X"));
        assert!(!hoisted_z.references.contains("Y"));

        // Check that D has Z@Y nested
        let d = result.dependencies.iter().find(|d| d.name == "D").unwrap();
        assert_eq!(d.dependencies.len(), 2);
        let nested_z = d.dependencies.iter().find(|d| d.name == "Z").unwrap();
        assert!(nested_z.references.contains("Y"));
        assert!(!nested_z.references.contains("X"));
    }

    #[test]
    fn should_hoist_dependencies_after_hoisting_peer_dep() {
        // . -> A -> B --> D@X
        //      -> D@X
        // should be hoisted to (B should be hoisted because its inherited dep D@X was hoisted):
        // . -> A
        //   -> B
        //   -> D@X
        let mut tree = IndexMap::new();
        tree.insert(".", TestNode {
            dependencies: vec!["A".to_string()],
            ..Default::default()
        });
        tree.insert("A", TestNode {
            dependencies: vec!["B".to_string(), "D@X".to_string()],
            ..Default::default()
        });
        tree.insert("B", TestNode {
            dependencies: vec!["D@X".to_string()],
            peer_names: vec!["D".to_string()],
            ..Default::default()
        });
        tree.insert("D@X", TestNode::default());

        let result = hoist(to_tree(tree), Some(HoistOptions {
            check: Some(true),
            ..Default::default()
        })).unwrap();

        assert_eq!(get_tree_height(&result), 2);
    }

    #[test]
    fn should_honor_unhoisted_peer_dependencies() {
        // . -> A --> B@X
        //        -> C@X -> B@Y
        //   -> B@X
        //   -> C@Y
        // should be hoisted to:
        // . -> A -> C@X -> B@Y
        //   -> B@X
        //   -> C@Y
        let mut tree = IndexMap::new();
        tree.insert(".", TestNode {
            dependencies: vec!["A".to_string(), "B@X".to_string(), "C@Y".to_string()],
            ..Default::default()
        });
        tree.insert("A", TestNode {
            dependencies: vec!["B@X".to_string(), "C@X".to_string()],
            peer_names: vec!["B".to_string()],
            ..Default::default()
        });
        tree.insert("C@X", TestNode {
            dependencies: vec!["B@Y".to_string()],
            ..Default::default()
        });
        tree.insert("B@X", TestNode::default());
        tree.insert("B@Y", TestNode::default());
        tree.insert("C@Y", TestNode::default());

        let result = hoist(to_tree(tree), Some(HoistOptions {
            check: Some(true),
            ..Default::default()
        })).unwrap();

        assert_eq!(get_tree_height(&result), 4);
    }

    #[test]
    fn should_honor_peer_dependency_promise_for_same_version() {
        // . -> A -> B -> C
        //   --> B
        // should be hoisted to (B must not be hoisted to the top):
        // . -> A -> B
        //   -> C
        let mut tree = IndexMap::new();
        tree.insert(".", TestNode {
            dependencies: vec!["A".to_string()],
            peer_names: vec!["B".to_string()],
            ..Default::default()
        });
        tree.insert("A", TestNode {
            dependencies: vec!["B".to_string()],
            ..Default::default()
        });
        tree.insert("B", TestNode {
            dependencies: vec!["C".to_string()],
            ..Default::default()
        });
        tree.insert("C", TestNode::default());

        let result = hoist(to_tree(tree), Some(HoistOptions {
            check: Some(true),
            ..Default::default()
        })).unwrap();

        assert_eq!(get_tree_height(&result), 3);
    }

    #[test]
    fn should_hoist_different_copies_independently() {
        // . -> A -> B@X -> C@X
        //        -> C@Y
        //   -> D -> B@X -> C@X
        //   -> B@Y
        //   -> C@Z
        // should be hoisted to (top C@X instance must not be hoisted):
        // . -> A -> B@X -> C@X
        //        -> C@Y
        //   -> D -> B@X
        //        -> C@X
        //   -> B@Y
        //   -> C@Z
        let mut tree = IndexMap::new();
        tree.insert(".", TestNode {
            dependencies: vec!["A".to_string(), "D".to_string(), "B@Y".to_string(), "C@Z".to_string()],
            ..Default::default()
        });
        tree.insert("A", TestNode {
            dependencies: vec!["B@X".to_string(), "C@Y".to_string()],
            ..Default::default()
        });
        tree.insert("B@X", TestNode {
            dependencies: vec!["C@X".to_string()],
            ..Default::default()
        });
        tree.insert("D", TestNode {
            dependencies: vec!["B@X".to_string()],
            ..Default::default()
        });
        tree.insert("B@Y", TestNode::default());
        tree.insert("C@X", TestNode::default());
        tree.insert("C@Y", TestNode::default());
        tree.insert("C@Z", TestNode::default());

        let result = hoist(to_tree(tree), Some(HoistOptions {
            check: Some(true),
            ..Default::default()
        })).unwrap();

        assert_eq!(get_tree_height(&result), 4);
    }

    #[test]
    fn should_keep_peer_dependency_promise_with_same_ident() {
        // . -> A -> B@X --> C
        //        -> C@Y
        //   -> B@X --> C
        //   -> C@X
        // B@X cannot be hoisted to the top from A, because its peer dependency promise will be violated
        let mut tree = IndexMap::new();
        tree.insert(".", TestNode {
            dependencies: vec!["A".to_string(), "B@X#2".to_string(), "C@X".to_string()],
            ..Default::default()
        });
        tree.insert("A", TestNode {
            dependencies: vec!["B@X#1".to_string(), "C@Y".to_string()],
            ..Default::default()
        });
        tree.insert("B@X#1", TestNode {
            dependencies: vec!["C@Y".to_string()],
            peer_names: vec!["C".to_string()],
            ident_name: Some("B".to_string()),
            ..Default::default()
        });
        tree.insert("B@X#2", TestNode {
            dependencies: vec!["C@X".to_string()],
            peer_names: vec!["C".to_string()],
            ident_name: Some("B".to_string()),
            ..Default::default()
        });
        tree.insert("C@X", TestNode::default());
        tree.insert("C@Y", TestNode::default());

        let result = hoist(to_tree(tree), Some(HoistOptions {
            check: Some(true),
            ..Default::default()
        })).unwrap();

        // Find A in the result
        let a = result.dependencies.iter().find(|d| d.name == "A").unwrap();
        // Check that B is still under A
        assert!(a.dependencies.iter().any(|d| d.name == "B"));
    }

    #[test]
    fn should_hoist_cyclic_peer_dependencies() {
        // Complex cyclic peer dependency test
        let mut tree = IndexMap::new();
        tree.insert(".", TestNode {
            dependencies: vec!["A".to_string()],
            ..Default::default()
        });
        tree.insert("A", TestNode {
            dependencies: vec!["B".to_string(), "C".to_string(), "D".to_string(), "E".to_string()],
            ..Default::default()
        });
        tree.insert("B", TestNode {
            dependencies: vec!["C".to_string(), "D".to_string(), "E".to_string(), "F".to_string(), "G".to_string()],
            peer_names: vec!["E".to_string()],
            ..Default::default()
        });
        tree.insert("C", TestNode {
            dependencies: vec!["D".to_string()],
            peer_names: vec!["D".to_string()],
            ..Default::default()
        });
        tree.insert("D", TestNode {
            dependencies: vec!["E".to_string(), "C".to_string()],
            peer_names: vec!["E".to_string(), "C".to_string()],
            ..Default::default()
        });
        tree.insert("E", TestNode {
            dependencies: vec!["C".to_string()],
            peer_names: vec!["C".to_string()],
            ..Default::default()
        });
        tree.insert("F", TestNode {
            dependencies: vec!["G".to_string()],
            peer_names: vec!["G".to_string()],
            ..Default::default()
        });
        tree.insert("G", TestNode::default());

        let result = hoist(to_tree(tree), Some(HoistOptions {
            check: Some(true),
            ..Default::default()
        })).unwrap();

        assert_eq!(get_tree_height(&result), 2);
    }

    #[test]
    fn should_not_hoist_past_hoist_boundary() {
        // . -> A -> B -> D
        //   -> C -> D
        // If B and C are hoist borders, the result should be:
        // . -> A
        //   -> B -> D
        //   -> C -> D
        let mut tree = IndexMap::new();
        tree.insert(".", TestNode {
            dependencies: vec!["A".to_string(), "C".to_string()],
            ..Default::default()
        });
        tree.insert("A", TestNode {
            dependencies: vec!["B".to_string()],
            ..Default::default()
        });
        tree.insert("B", TestNode {
            dependencies: vec!["D".to_string()],
            ..Default::default()
        });
        tree.insert("C", TestNode {
            dependencies: vec!["D".to_string()],
            ..Default::default()
        });
        tree.insert("D", TestNode::default());

        let mut hoisting_limits = IndexMap::new();
        hoisting_limits.insert(".@".to_string(), ["C".to_string()].into_iter().collect());
        hoisting_limits.insert("A@".to_string(), ["B".to_string()].into_iter().collect());

        let result = hoist(to_tree(tree), Some(HoistOptions {
            check: Some(true),
            hoisting_limits: Some(hoisting_limits),
            ..Default::default()
        })).unwrap();

        assert_eq!(get_tree_height(&result), 3);
    }

    #[test]
    fn should_hoist_workspace_dependencies() {
        // . -> W1(w) -> W2(w) -> W3(w)-> A@X
        //            -> A@Y
        //   -> W3
        //   -> A@Z
        // The A@X must be hoisted into W2(w)
        let mut tree = IndexMap::new();
        tree.insert(".", TestNode {
            dependencies: vec!["W1(w)".to_string(), "W3".to_string(), "A@Z".to_string()],
            dependency_kind: Some(HoisterDependencyKind::Workspace),
            ..Default::default()
        });
        tree.insert("W1(w)", TestNode {
            dependencies: vec!["W2(w)".to_string(), "A@Y".to_string()],
            dependency_kind: Some(HoisterDependencyKind::Workspace),
            ident_name: Some("W1".to_string()),
            ..Default::default()
        });
        tree.insert("W2(w)", TestNode {
            dependencies: vec!["W3(w)".to_string()],
            dependency_kind: Some(HoisterDependencyKind::Workspace),
            ident_name: Some("W2".to_string()),
            ..Default::default()
        });
        tree.insert("W3(w)", TestNode {
            dependencies: vec!["A@X".to_string()],
            dependency_kind: Some(HoisterDependencyKind::Workspace),
            ident_name: Some("W3".to_string()),
            ..Default::default()
        });
        tree.insert("W3", TestNode::default());
        tree.insert("A@X", TestNode::default());
        tree.insert("A@Y", TestNode::default());
        tree.insert("A@Z", TestNode::default());

        let result = hoist(to_tree(tree), Some(HoistOptions {
            check: Some(true),
            ..Default::default()
        })).unwrap();

        assert_eq!(get_tree_height(&result), 4);
    }

    #[test]
    fn should_hoist_aliased_packages() {
        let mut tree = IndexMap::new();
        tree.insert(".", TestNode {
            dependencies: vec!["Aalias".to_string()],
            ..Default::default()
        });
        tree.insert("Aalias", TestNode {
            ident_name: Some("A".to_string()),
            dependencies: vec!["A".to_string()],
            ..Default::default()
        });
        tree.insert("A", TestNode {
            dependencies: vec!["B".to_string()],
            ..Default::default()
        });
        tree.insert("B", TestNode::default());

        let result = hoist(to_tree(tree), Some(HoistOptions {
            check: Some(true),
            ..Default::default()
        })).unwrap();

        assert_eq!(get_tree_height(&result), 3);
    }

    #[test]
    fn should_not_hoist_portal_with_unhoistable_deps() {
        let mut tree = IndexMap::new();
        tree.insert(".", TestNode {
            dependencies: vec!["P1".to_string(), "B@Y".to_string()],
            ..Default::default()
        });
        tree.insert("P1", TestNode {
            dependencies: vec!["P2".to_string()],
            dependency_kind: Some(HoisterDependencyKind::ExternalSoftLink),
            ..Default::default()
        });
        tree.insert("P2", TestNode {
            dependencies: vec!["B@X".to_string()],
            dependency_kind: Some(HoisterDependencyKind::ExternalSoftLink),
            ..Default::default()
        });
        tree.insert("B@X", TestNode::default());
        tree.insert("B@Y", TestNode::default());

        let result = hoist(to_tree(tree), Some(HoistOptions {
            check: Some(true),
            ..Default::default()
        })).unwrap();

        assert_eq!(get_tree_height(&result), 3);
    }

    #[test]
    fn should_hoist_nested_portals_with_hoisted_deps() {
        let mut tree = IndexMap::new();
        tree.insert(".", TestNode {
            dependencies: vec!["P1".to_string(), "B@X".to_string()],
            ..Default::default()
        });
        tree.insert("P1", TestNode {
            dependencies: vec!["P2".to_string(), "B@X".to_string()],
            dependency_kind: Some(HoisterDependencyKind::ExternalSoftLink),
            ..Default::default()
        });
        tree.insert("P2", TestNode {
            dependencies: vec!["P3".to_string(), "B@X".to_string()],
            dependency_kind: Some(HoisterDependencyKind::ExternalSoftLink),
            ..Default::default()
        });
        tree.insert("P3", TestNode {
            dependencies: vec!["B@X".to_string()],
            dependency_kind: Some(HoisterDependencyKind::ExternalSoftLink),
            ..Default::default()
        });
        tree.insert("B@X", TestNode::default());

        let result = hoist(to_tree(tree), Some(HoistOptions {
            check: Some(true),
            ..Default::default()
        })).unwrap();

        assert_eq!(get_tree_height(&result), 2);
    }

    #[test]
    fn should_support_two_branch_circular_graph() {
        // . -> B -> D@X -> F@X
        //               -> E@X -> D@X
        //                      -> F@X
        //   -> C -> D@Y -> F@Y
        //               -> E@Y -> D@Y
        //                      -> F@Y
        let mut tree = IndexMap::new();
        tree.insert(".", TestNode {
            dependencies: vec!["B".to_string(), "C".to_string()],
            ..Default::default()
        });
        tree.insert("B", TestNode {
            dependencies: vec!["D@X".to_string()],
            ..Default::default()
        });
        tree.insert("C", TestNode {
            dependencies: vec!["D@Y".to_string()],
            ..Default::default()
        });
        tree.insert("D@X", TestNode {
            dependencies: vec!["E@X".to_string(), "F@X".to_string()],
            ..Default::default()
        });
        tree.insert("D@Y", TestNode {
            dependencies: vec!["E@Y".to_string(), "F@X".to_string()],
            ..Default::default()
        });
        tree.insert("E@X", TestNode {
            dependencies: vec!["D@X".to_string(), "F@X".to_string()],
            ..Default::default()
        });
        tree.insert("E@Y", TestNode {
            dependencies: vec!["D@Y".to_string(), "F@Y".to_string()],
            ..Default::default()
        });
        tree.insert("F@X", TestNode::default());
        tree.insert("F@Y", TestNode::default());

        let result = hoist(to_tree(tree), Some(HoistOptions {
            check: Some(true),
            ..Default::default()
        })).unwrap();

        assert_eq!(get_tree_height(&result), 4);
    }
}
