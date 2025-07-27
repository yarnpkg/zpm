use itertools::Either;

#[derive(Clone)]
pub struct Node {
    pub label: String,
    pub children: Vec<Node>,
}

pub trait RenderTreeNode: Sized {
    fn get_label(&self) -> String;
    fn has_children(&self) -> bool;
    fn get_children(&self) -> Vec<Either<Node, Self>>;
}

impl RenderTreeNode for Node {
    fn get_label(&self) -> String {
        self.label.clone()
    }

    fn has_children(&self) -> bool {
        !self.children.is_empty()
    }

    fn get_children(&self) -> Vec<Either<Node, Self>> {
        self.children.clone().into_iter().map(Either::Left).collect()
    }
}

pub struct TreeRenderer {
    prefix: String,
}

impl TreeRenderer {
    pub fn new() -> Self {
        Self {prefix: "".to_string()}
    }

    pub fn render<T: RenderTreeNode>(&mut self, node: &T) -> String {
        let mut result = String::new();

        let label
            = node.get_label();

        if self.prefix.len() > 0 || label.len() > 0 {
            let mut lines
                = label.lines();

            let first_line
                = lines.next().unwrap_or("");

            result.push_str(&first_line);
            result.push('\n');

            for line in lines {
                result.push_str(&self.prefix);
                result.push_str(line);
                result.push('\n');
            }
        }

        if node.has_children() {
            let children
                = node.get_children();

            for (index, child) in children.iter().enumerate() {
                let is_last = index == children.len() - 1;

                // Choose the appropriate characters based on whether this is the last child
                let (connector, mut next_prefix) = if is_last {
                    ("└─ ", self.prefix.clone() + "   ")
                } else {
                    ("├─ ", self.prefix.clone() + "│  ")
                };

                // Check if we need to add a newline between children
                if self.prefix.len() == 0 && index > 0 {
                    let prev_child_has_children = match &children[index - 1] {
                        Either::Left(node) => node.has_children(),
                        Either::Right(node) => node.has_children(),
                    };

                    let current_child_has_children = match child {
                        Either::Left(node) => node.has_children(),
                        Either::Right(node) => node.has_children(),
                    };

                    if prev_child_has_children || current_child_has_children {
                        // Add a newline with a vertical bar to maintain the tree structure
                        result.push_str("│\n");
                    }
                }

                // Add the connector and child representation
                result.push_str(&self.prefix);
                result.push_str(connector);

                std::mem::swap(&mut self.prefix, &mut next_prefix);

                result.push_str(&match child {
                    Either::Left(node) => self.render(node),
                    Either::Right(node) => self.render(node),
                });

                std::mem::swap(&mut self.prefix, &mut next_prefix);
            }
        }

        result
    }
}

impl Node {
    pub fn to_string(&self) -> String {
        TreeRenderer::new().render(self)
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

    #[test]
    fn test_multiline_at_start() {
        let node = Node {
            label: "Root".to_string(),
            children: vec![
                Node {
                    label: "Multi-line child\nSecond line\nThird line".to_string(),
                    children: vec![],
                },
                Node {
                    label: "Child 2".to_string(),
                    children: vec![],
                },
                Node {
                    label: "Child 3".to_string(),
                    children: vec![],
                },
            ],
        };

        let expected = "\
Root
├─ Multi-line child
│  Second line
│  Third line
├─ Child 2
└─ Child 3
";

        assert_eq!(node.to_string(), expected);
    }

    #[test]
    fn test_multiline_at_end() {
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
                Node {
                    label: "Last child with\nmultiple lines\nof text".to_string(),
                    children: vec![],
                },
            ],
        };

        let expected = "\
Root
├─ Child 1
├─ Child 2
└─ Last child with
   multiple lines
   of text
";

        assert_eq!(node.to_string(), expected);
    }
}
