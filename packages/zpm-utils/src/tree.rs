use colored::Colorize;
use indexmap::IndexMap;
use serde::{ser::SerializeMap, Serialize};

use crate::{AbstractValue, Extracted, ToHumanString};

pub type Map<'a> = IndexMap<String, Node<'a>>;

#[derive(Debug)]
pub enum TreeNodeChildren<'a> {
    Vec(Vec<Node<'a>>),
    Map(IndexMap<String, Node<'a>>),
}

impl<'a> TreeNodeChildren<'a> {
    fn is_empty(&self) -> bool {
        match self {
            TreeNodeChildren::Vec(children) =>
                children.is_empty(),

            TreeNodeChildren::Map(children) =>
                children.is_empty(),
        }
    }

    fn len(&self) -> usize {
        match self {
            TreeNodeChildren::Vec(children) =>
                children.len(),

            TreeNodeChildren::Map(children) =>
                children.len(),
        }
    }

    fn iter(&self) -> Box<dyn Iterator<Item = &Node<'a>> + '_> {
        match self {
            TreeNodeChildren::Vec(children) =>
                Box::new(children.iter()),

            TreeNodeChildren::Map(children) =>
                Box::new(children.values()),
        }
    }
}

impl<'a> Serialize for TreeNodeChildren<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where S: serde::Serializer {
        match self {
            TreeNodeChildren::Vec(children) =>
                children.serialize(serializer),

            TreeNodeChildren::Map(children) =>
                children.serialize(serializer),
        }
    }
}

#[derive(Debug)]
pub struct Node<'a> {
    pub label: Option<String>,
    pub value: Option<AbstractValue<'a>>,
    pub children: Option<TreeNodeChildren<'a>>,
}

impl<'a> Node<'a> {
    pub fn new_value<T: Extracted + 'a>(value: T) -> Self {
        Self {
            label: None,
            value: Some(AbstractValue::new(value)),
            children: None,
        }
    }

    pub fn render_line(&self) -> String {
        if let Some(label) = &self.label {
            let mut result
                = label.clone();

            if self.value.is_some() {
                result.push(':');
                result.push(' ');
            }

            if self.value.is_some() || self.children.is_some() {
                result = result.bold().to_string();
            }

            if let Some(value) = &self.value {
                result.push_str(&value.to_print_string());
            }

            result
        } else if let Some(value) = &self.value {
            value.to_print_string()
        } else {
            "".to_string()
        }
    }
}

impl<'a> Serialize for Node<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where S: serde::Serializer {
        if let Some(children) = &self.children {
            if let Some(value) = &self.value {
                let mut map
                    = serializer.serialize_map(Some(2))?;

                map.serialize_entry("value", value)?;
                map.serialize_entry("children", children)?;

                map.end()
            } else {
                children.serialize(serializer)
            }
        } else if let Some(value) = &self.value {
            value.serialize(serializer)
        } else {
            serializer.serialize_none()
        }
    }
}

pub struct TreeRenderer {
    prefix: String,
}

impl TreeRenderer {
    pub fn new() -> Self {
        Self {prefix: "".to_string()}
    }

    pub fn render<'a>(&mut self, node: &Node<'a>, json: bool) -> String {
        if json {
            self.render_json(node)
        } else {
            self.render_text(node)
        }
    }

    pub fn render_json<'a>(&mut self, node: &Node<'a>) -> String {
        let mut result
            = String::new();

        if let Some(children) = &node.children {
            for child in children.iter() {
                result.push_str(&crate::internal::to_json_string(child));
                result.push('\n');
            }
        }

        result
    }

    pub fn render_text<'a>(&mut self, node: &Node<'a>) -> String {
        let mut result
            = String::new();

        let label
            = node.render_line();

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

        if let Some(children) = &node.children {
            if !children.is_empty() {
                let mut previous_child: Option<&Node<'a>>
                    = None;

                let last_index
                    = children.len() - 1;

                for (index, child) in children.iter().enumerate() {
                    let is_last
                        = index == last_index;

                    // Choose the appropriate characters based on whether this is the last child
                    let (connector, mut next_prefix) = if is_last {
                        ("└─ ", self.prefix.clone() + "   ")
                    } else {
                        ("├─ ", self.prefix.clone() + "│  ")
                    };

                    // Check if we need to add a newline between children
                    if let Some(previous_child) = previous_child {
                        let prev_child_has_children
                            = previous_child.children
                                .as_ref()
                                .map_or(false, |children| !children.is_empty());

                        let current_child_has_children
                            = child.children
                                .as_ref()
                                .map_or(false, |children| !children.is_empty());

                        if prev_child_has_children || current_child_has_children {
                            // Add a newline with a vertical bar to maintain the tree structure
                            result.push_str(&self.prefix);
                            result.push_str("│\n");
                        }
                    }

                    // Add the connector and child representation
                    result.push_str(&self.prefix);
                    result.push_str(connector);

                    std::mem::swap(&mut self.prefix, &mut next_prefix);

                    result.push_str(&self.render_text(&child));

                    std::mem::swap(&mut self.prefix, &mut next_prefix);

                    previous_child = Some(child);
                }
            }
        }

        result
    }
}

impl<'a> Node<'a> {
    pub fn to_string(&self) -> String {
        TreeRenderer::new().render_text(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_single_node() {
        let node = Node {
            label: Some("Root".to_string()),
            value: None,
            children: None,
        };

        assert_eq!(node.to_string(), "Root\n");
    }

    #[test]
    fn test_node_with_children() {
        let node = Node {
            label: Some("Root".to_string()),
            value: None,
            children: Some(TreeNodeChildren::Vec(vec![
                Node {
                    label: Some("Child 1".to_string()),
                    value: None,
                    children: None,
                },
                Node {
                    label: Some("Child 2".to_string()),
                    value: None,
                    children: None,
                },
            ])),
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
            label: Some("Root".to_string()),
            value: None,
            children: Some(TreeNodeChildren::Vec(vec![
                Node {
                    label: Some("Child 1".to_string()),
                    value: None,
                    children: Some(TreeNodeChildren::Vec(vec![
                        Node {
                            label: Some("Grandchild 1".to_string()),
                            value: None,
                            children: None,
                        },
                    ])),
                },
                Node {
                    label: Some("Child 2".to_string()),
                    value: None,
                    children: None,
                },
                Node {
                    label: Some("Child 3".to_string()),
                    value: None,
                    children: Some(TreeNodeChildren::Vec(vec![
                        Node {
                            label: Some("Grandchild 2".to_string()),
                            value: None,
                            children: None,
                        },
                    ])),
                },
            ])),
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
            label: Some("Root".to_string()),
            value: None,
            children: Some(TreeNodeChildren::Vec(vec![
                Node {
                    label: Some("Child 1".to_string()),
                    value: None,
                    children: Some(TreeNodeChildren::Vec(vec![
                        Node {
                            label: Some("Grandchild 1".to_string()),
                            value: None,
                            children: None,
                        },
                        Node {
                            label: Some("Grandchild 2".to_string()),
                            value: None,
                            children: None,
                        },
                    ])),
                },
                Node {
                    label: Some("Child 2".to_string()),
                    value: None,
                    children: Some(TreeNodeChildren::Vec(vec![
                        Node {
                            label: Some("Grandchild 3".to_string()),
                            value: None,
                            children: None,
                        },
                    ])),
                },
                Node {
                    label: Some("Child 3".to_string()),
                    value: None,
                    children: None,
                },
            ])),
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
            label: Some("Root".to_string()),
            value: None,
            children: Some(TreeNodeChildren::Vec(vec![
                Node {
                    label: Some("Child 1".to_string()),
                    value: None,
                    children: Some(TreeNodeChildren::Vec(vec![
                        Node {
                            label: Some("Grandchild 1".to_string()),
                            value: None,
                            children: Some(TreeNodeChildren::Vec(vec![
                                Node {
                                    label: Some("Great-grandchild 1".to_string()),
                                    value: None,
                                    children: None,
                                },
                            ])),
                        }
                    ])),
                },
                Node {
                    label: Some("Child 2".to_string()),
                    value: None,
                    children: None,
                },
            ])),
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
            label: Some("Root".to_string()),
            value: None,
            children: Some(TreeNodeChildren::Vec(vec![
                Node {
                    label: Some("Multi-line child\nSecond line\nThird line".to_string()),
                    value: None,
                    children: None,
                },
                Node {
                    label: Some("Child 2".to_string()),
                    value: None,
                    children: None,
                },
                Node {
                    label: Some("Child 3".to_string()),
                    value: None,
                    children: None,
                },
            ])),
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
            label: Some("Root".to_string()),
            value: None,
            children: Some(TreeNodeChildren::Vec(vec![
                Node {
                    label: Some("Child 1".to_string()),
                    value: None,
                    children: None,
                },
                Node {
                    label: Some("Child 2".to_string()),
                    value: None,
                    children: None,
                },
                Node {
                    label: Some("Last child with\nmultiple lines\nof text".to_string()),
                    value: None,
                    children: None,
                },
            ])),
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
