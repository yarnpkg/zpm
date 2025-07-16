use std::collections::HashMap;

use crate::{node::Field, Formatter, Path, Value};

fn insert_path(current: &mut Value, path: &Path, value: Value) {
    if path.is_empty() {
        *current = value;
        return;
    }

    // We only ever descend through objects.
    let Value::Object(ref mut fields) = current else {
        panic!("attempted to descend into a non‑object value");
    };

    let head = &path[0];
    let tail = &path[1..];

    // Do we already have an entry for this key?
    match fields.iter_mut().find(|(k, _)| k == head) {
        Some((_, child)) => {
            // Recurse into the existing child
            insert_path(child, &Path::from_segments(tail.to_vec()), value);
        },

        None => {
            // Create a fresh subtree and push it
            let mut new_child = Value::Object(vec![]);
            insert_path(&mut new_child, &Path::from_segments(tail.to_vec()), value);
            fields.push((head.clone(), new_child));
        },
    }
}

fn expand_fields_to_value(pairs: &[(Path, Value)]) -> Value {
    let mut root = Value::Object(vec![]);

    for (path, leaf) in pairs {
        insert_path(&mut root, path, leaf.clone());
    }

    root
}

#[derive(Debug, PartialEq, Eq)]
pub struct Update {
    pub offset: usize,
    pub size: usize,
    pub data: String,
}

#[derive(Debug, PartialEq, Eq)]
pub struct UpdateSet {
    pub updates: Vec<Update>,
}

impl UpdateSet {
    pub fn apply_to_document(&self, document: &str) -> String {
        let mut result
            = document.to_string();

        let mut offset
            = 0isize;

        for update in self.updates.iter() {
            let start
                = update.offset as isize + offset;
            let end
                = start + update.size as isize;

            result.replace_range(start as usize..end as usize, &update.data);

            offset += update.data.len() as isize - update.size as isize;
        }

        result.to_string()
    }
}

pub struct Ops {
    set: HashMap<Path, Value>,
}

impl Ops {
    pub fn new() -> Self {
        Self {
            set: HashMap::new(),
        }
    }

    pub fn set(&mut self, path: Path, value: Value) {
        self.set.insert(path, value);
    }

    pub fn derive<F: Formatter>(&self, fields: &[Field]) -> UpdateSet {
        let mut preferred_indent
            = 2usize;

        let mut updates
            = Vec::new();

        let mut field_map
            = HashMap::new();

        for field in fields {
            field_map.insert(field.path.clone(), field);
        }

        // We extract all fields that are not already present in the document. We'll
        // need to inject them after collecting their child properties.

        #[derive(Debug)]
        struct MissingField {
            child_values: Vec<(Path, Value)>,
        }

        let mut missing_fields: HashMap<Path, MissingField>
            = HashMap::new();

        for (path, value) in self.set.iter() {
            if value != &Value::Undefined {
                for len in 0..path.len() {
                    let segment_path
                        = Path::from_segments(path[0..=len].to_vec());
                    let paren_path
                        = Path::from_segments(path[0..len].to_vec());

                    if field_map.get(&segment_path).is_none() {
                        missing_fields.entry(paren_path)
                            .or_insert_with(|| MissingField {
                                child_values: Vec::new(),
                            })
                            .child_values.push((Path::from_segments(path[len..].to_vec()), value.clone()));

                        break;
                    }
                }
            }
        }

        let mut current_indent
            = 0usize;

        for (path, mut missing_field) in missing_fields {
            let parent_field
                = field_map.get(&path)
                    .expect("Parent block should be present since they weren't missing");

            missing_field.child_values.sort_by_key(|(path, _)| path.clone());

            updates.push(Update {
                offset: parent_field.node.offset + parent_field.node.size,
                size: 0,
                data: F::value_to_string(&expand_fields_to_value(&missing_field.child_values), preferred_indent, parent_field.node.indent),
            });
        }

        for field in fields {
            if field.node.indent > current_indent {
                preferred_indent = field.node.indent - current_indent;
            }

            current_indent = field.node.indent;

            let update
                = self.set.get(&field.path);

            let Some(update) = update else {
                continue;
            };

            if update == &Value::Undefined {
                updates.push(Update {
                    offset: field.node.offset,
                    size: field.node.size,
                    data: "".to_string(),
                });

                continue;
            }

            updates.push(Update {
                offset: field.node.offset,
                size: field.node.size,
                data: F::value_to_string(update, preferred_indent, current_indent),
            });
        }

        updates.sort_by_key(|update| {
            update.offset
        });

        UpdateSet {
            updates,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{yaml_formatter::YamlFormatter, yaml_parser};

    use super::*;

    #[test]
    fn derive_simple_update() {
        let mut ops = Ops::new();

        ops.set(Path::from_segments(vec!["test".to_string()]), Value::String("foo".to_string()));

        let fields = yaml_parser::YamlParser::new("test: value\n".as_bytes())
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(ops.derive::<YamlFormatter>(&fields), UpdateSet {
            updates: vec![
                Update {
                    offset: 6,
                    size: 5,
                    data: "foo".to_string(),
                },
            ],
        });
    }

    #[test]
    fn derive_multiple_updates_same_field() {
        let mut ops = Ops::new();

        ops.set(Path::from_segments(vec!["test".to_string()]), Value::String("foo".to_string()));
        ops.set(Path::from_segments(vec!["test".to_string()]), Value::String("bar".to_string()));

        let fields = yaml_parser::YamlParser::new("test: value\n".as_bytes())
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(ops.derive::<YamlFormatter>(&fields), UpdateSet {
            updates: vec![
                Update {
                    offset: 6,
                    size: 5,
                    data: "bar".to_string(),
                },
            ],
        });
    }

    #[test]
    fn derive_multiple_updates_different_fields() {
        let mut ops = Ops::new();

        ops.set(Path::from_segments(vec!["test".to_string()]), Value::String("foo".to_string()));
        ops.set(Path::from_segments(vec!["test2".to_string()]), Value::String("bar".to_string()));

        let fields = yaml_parser::YamlParser::new("test: value\ntest2: value2\n".as_bytes())
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(ops.derive::<YamlFormatter>(&fields), UpdateSet {
            updates: vec![
                Update {
                    offset: 6,
                    size: 5,
                    data: "foo".to_string(),
                },
                Update {
                    offset: 19,
                    size: 6,
                    data: "bar".to_string(),
                },
            ],
        });
    }

    #[test]
    fn derive_new_field_empty() {
        let mut ops = Ops::new();

        ops.set(Path::from_segments(vec!["test2".to_string()]), Value::String("foo".to_string()));

        let fields = yaml_parser::YamlParser::new("".as_bytes())
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(ops.derive::<YamlFormatter>(&fields), UpdateSet {
            updates: vec![
                Update {
                    offset: 0,
                    size: 0,
                    data: "\ntest2: foo".to_string(),
                },
            ],
        });
    }

    #[test]
    fn derive_new_field_end() {
        let mut ops = Ops::new();

        ops.set(Path::from_segments(vec!["test2".to_string()]), Value::String("foo".to_string()));

        let fields = yaml_parser::YamlParser::new("test: value\n".as_bytes())
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(ops.derive::<YamlFormatter>(&fields), UpdateSet {
            updates: vec![
                Update {
                    offset: 11,
                    size: 0,
                    data: "\ntest2: foo".to_string(),
                },
            ],
        });
    }

    #[test]
    fn derive_new_field_nested_exists() {
        let mut ops = Ops::new();

        ops.set(Path::from_segments(vec!["test".to_string(), "test2".to_string()]), Value::String("foo".to_string()));

        let fields = yaml_parser::YamlParser::new("test:\n  foo: value\n".as_bytes())
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(ops.derive::<YamlFormatter>(&fields), UpdateSet {
            updates: vec![
                Update {
                    offset: 18,
                    size: 0,
                    data: "\n  test2: foo".to_string(),
                },
            ],
        });
    }

    #[test]
    fn derive_new_field_nested_missing() {
        let mut ops = Ops::new();

        ops.set(Path::from_segments(vec!["test".to_string(), "test2".to_string()]), Value::String("foo".to_string()));

        let fields = yaml_parser::YamlParser::new("foo:\n".as_bytes())
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(ops.derive::<YamlFormatter>(&fields), UpdateSet {
            updates: vec![
                Update {
                    offset: 4,
                    size: 0,
                    data: "\ntest:\n  test2: foo".to_string(),
                },
            ],
        });
    }

    #[test]
    fn apply_to_document_simple_update() {
        assert_eq!(UpdateSet {
            updates: vec![
                Update {
                    offset: 6,
                    size: 5,
                    data: "foo".to_string(),
                },
            ],
        }.apply_to_document("test: value\n"), "test: foo\n");
    }

    #[test]
    fn apply_to_document_multiple_updates() {
        assert_eq!(UpdateSet {
            updates: vec![
                Update {
                    offset: 6,
                    size: 5,
                    data: "foo".to_string(),
                },
                Update {
                    offset: 19,
                    size: 6,
                    data: "bar".to_string(),
                },
            ],
        }.apply_to_document("test: value\ntest2: value2\n"), "test: foo\ntest2: bar\n");
    }

    #[test]
    fn apply_to_document_single_removal() {
        assert_eq!(UpdateSet {
            updates: vec![
                Update {
                    offset: 12,
                    size: 14,
                    data: "".to_string(),
                },
            ],
        }.apply_to_document("test: value\ntest2: value2\ntest3: value3\n"), "test: value\ntest3: value3\n");
    }

    #[test]
    fn apply_to_document_multiple_removals() {
        assert_eq!(UpdateSet {
            updates: vec![
                Update {
                    offset: 12,
                    size: 14,
                    data: "".to_string(),
                },
                Update {
                    offset: 40,
                    size: 14,
                    data: "".to_string(),
                },
            ],
        }.apply_to_document("test: value\ntest2: value2\ntest3: value3\ntest4: value4\ntest5: value5\n"), "test: value\ntest3: value3\ntest5: value5\n");
    }
}
