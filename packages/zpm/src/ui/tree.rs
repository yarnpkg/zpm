pub struct Node {
    pub label: String,
    pub children: Vec<Node>,
}

impl Node {
    pub fn to_string(&self) -> String {
        self.to_string_with_prefix("".to_string())
    }

    fn to_string_with_prefix(&self, prefix: String) -> String {
        let mut result = String::new();

        if prefix.len() > 0 || self.label.len() > 0 {
            let mut lines
                = self.label.lines();

            let first_line
                = lines.next().unwrap_or("");

            result.push_str(&first_line);
            result.push('\n');

            for line in lines {
                result.push_str(&prefix);
                result.push_str(line);
                result.push('\n');
            }
        }

        let children_count = self.children.len();
        for (index, child) in self.children.iter().enumerate() {
            let is_last = index == children_count - 1;
            
            // Choose the appropriate characters based on whether this is the last child
            let (connector, next_prefix) = if is_last {
                ("└─ ", prefix.clone() + "   ")
            } else {
                ("├─ ", prefix.clone() + "│  ")
            };

            // Check if we need to add a newline between children
            if prefix.len() == 0 && index > 0 {
                let prev_child_has_children = !self.children[index - 1].children.is_empty();
                let current_child_has_children = !child.children.is_empty();
                
                if prev_child_has_children || current_child_has_children {
                    // Add a newline with a vertical bar to maintain the tree structure
                    result.push_str("│\n");
                }
            }

            // Add the connector and child representation
            result.push_str(&prefix);
            result.push_str(connector);
            result.push_str(&child.to_string_with_prefix(next_prefix));
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_single_node() {
        let node = Node {
            label: "Root".to_string(),
            children: vec![],
        };

        assert_eq!(node.to_string(), "Root\n");
    }

    #[test]
    fn test_node_with_children() {
        let node = Node {
            label: "Root".to_string(),
            children: vec![
                Node {
                    label: "Child 1".to_string(),
                    children: vec![],
                },
                Node {
                    label: "Child 2".to_string(),
                    children: vec![],
                },
            ],
        };

        // No newlines between siblings without children
        let expected = "\
Root
├─ Child 1
└─ Child 2
";

        assert_eq!(node.to_string(), expected);
    }

    #[test]
    fn test_node_with_children_requiring_newlines() {
        let node = Node {
            label: "Root".to_string(),
            children: vec![
                Node {
                    label: "Child 1".to_string(),
                    children: vec![
                        Node {
                            label: "Grandchild 1".to_string(),
                            children: vec![],
                        },
                    ],
                },
                Node {
                    label: "Child 2".to_string(),
                    children: vec![],
                },
                Node {
                    label: "Child 3".to_string(),
                    children: vec![
                        Node {
                            label: "Grandchild 2".to_string(),
                            children: vec![],
                        },
                    ],
                },
            ],
        };

        // Newlines between siblings with proper indentation
        let expected = "\
Root
├─ Child 1
│  └─ Grandchild 1
│
├─ Child 2
│
└─ Child 3
   └─ Grandchild 2
";

        assert_eq!(node.to_string(), expected);
    }

    #[test]
    fn test_complex_tree() {
        let node = Node {
            label: "Root".to_string(),
            children: vec![
                Node {
                    label: "Child 1".to_string(),
                    children: vec![
                        Node {
                            label: "Grandchild 1".to_string(),
                            children: vec![],
                        },
                        Node {
                            label: "Grandchild 2".to_string(),
                            children: vec![],
                        },
                    ],
                },
                Node {
                    label: "Child 2".to_string(),
                    children: vec![
                        Node {
                            label: "Grandchild 3".to_string(),
                            children: vec![],
                        },
                    ],
                },
                Node {
                    label: "Child 3".to_string(),
                    children: vec![],
                },
            ],
        };

        let expected = "\
Root
├─ Child 1
│  ├─ Grandchild 1
│  └─ Grandchild 2
│
├─ Child 2
│  └─ Grandchild 3
│
└─ Child 3
";

        assert_eq!(node.to_string(), expected);
    }

    #[test]
    fn test_deeply_nested_tree() {
        let node = Node {
            label: "Root".to_string(),
            children: vec![
                Node {
                    label: "Child 1".to_string(),
                    children: vec![
                        Node {
                            label: "Grandchild 1".to_string(),
                            children: vec![
                                Node {
                                    label: "Great-grandchild 1".to_string(),
                                    children: vec![],
                                },
                            ],
                        },
                    ],
                },
                Node {
                    label: "Child 2".to_string(),
                    children: vec![],
                },
            ],
        };

        let expected = "\
Root
├─ Child 1
│  └─ Grandchild 1
│     └─ Great-grandchild 1
│
└─ Child 2
";

        assert_eq!(node.to_string(), expected);
    }
}
