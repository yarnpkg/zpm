use serde::Serialize;

use crate::{Error, JsonDocument, json::json_provider};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IndentStyle {
    #[default]
    Spaces,
    Tabs,
}

impl IndentStyle {
    pub fn char(&self) -> &'static str {
        match self {
            IndentStyle::Spaces => " ",
            IndentStyle::Tabs => "\t",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Indent {
    pub self_indent: Option<usize>,
    pub child_indent: Option<usize>,
    pub style: IndentStyle,
}

impl Indent {
    pub fn new(self_indent: Option<usize>, child_indent: Option<usize>) -> Self {
        Self {
            self_indent,
            child_indent,
            style: IndentStyle::default(),
        }
    }

    pub fn with_style(self_indent: Option<usize>, child_indent: Option<usize>, style: IndentStyle) -> Self {
        Self {
            self_indent,
            child_indent,
            style,
        }
    }

    pub fn increment(&self) -> Self {
        let self_indent
            = self.child_indent;

        let child_indent = match self.child_indent {
            Some(i) => Some(i + self.self_indent.unwrap_or(i)),
            None => None,
        };

        Self {
            self_indent,
            child_indent,
            style: self.style,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Number(String), // Store as string to preserve exact formatting
    String(String),
    Array(Vec<Value>),
    Object(Vec<(String, Value)>), // Preserves insertion order
    Undefined, // Used to remove values
    Raw(String), // Used to store raw JSON or YAML; not portable
}

impl Value {
    pub fn from_serializable<T: Serialize>(value: &T) -> Result<Self, Error> {
        Ok(Value::from(&json_provider::to_value(value)?))
    }

    pub fn to_json_string(&self) -> String {
        self.to_indented_json_string(Indent::new(None, None))
    }

    pub fn to_indented_json_string(&self, indent: Indent) -> String {
        match self {
            Value::Null => {
                "null".to_string()
            },

            Value::Bool(b) => {
                b.to_string()
            },

            Value::Number(n) => {
                n.to_string()
            },

            Value::String(s) => {
                JsonDocument::to_string(s).expect("Failed to convert string to JSON")
            },

            Value::Array(arr) => {
                let mut serializer
                    = String::new();

                let indent_char = indent.style.char();

                serializer.push_str("[");

                for (i, item) in arr.iter().enumerate() {
                    if let Some(child_indent) = indent.child_indent {
                        serializer.push_str("\n");
                        for _ in 0..child_indent {
                            serializer.push_str(indent_char);
                        }
                    } else if i > 0 {
                        serializer.push(' ');
                    }

                    serializer.push_str(&item.to_indented_json_string(indent.increment()));

                    if i < arr.len() - 1 {
                        serializer.push_str(",");
                    }
                }

                if !arr.is_empty() {
                    if indent.child_indent.is_some() {
                        serializer.push_str("\n");
                        if let Some(child_indent) = indent.self_indent {
                            for _ in 0..child_indent {
                                serializer.push_str(indent_char);
                            }
                        }
                    }
                }

                serializer.push(']');

                serializer
            },

            Value::Object(obj) => {
                let mut serializer
                    = String::new();

                let indent_char = indent.style.char();

                serializer.push_str("{");

                for (i, (k, v)) in obj.iter().enumerate() {
                    if let Some(child_indent) = indent.child_indent {
                        serializer.push_str("\n");
                        for _ in 0..child_indent {
                            serializer.push_str(indent_char);
                        }
                    } else if i > 0 {
                        serializer.push(' ');
                    }

                    serializer.push_str(&JsonDocument::to_string(k).expect("Failed to convert key to JSON"));
                    serializer.push_str(": ");
                    serializer.push_str(&v.to_indented_json_string(indent.increment()));

                    if i < obj.len() - 1 {
                        serializer.push_str(",");
                    }
                }

                if !obj.is_empty() {
                    if indent.child_indent.is_some() {
                        serializer.push_str("\n");
                        if let Some(child_indent) = indent.self_indent {
                            for _ in 0..child_indent {
                                serializer.push_str(indent_char);
                            }
                        }
                    }
                }

                serializer.push('}');

                serializer
            },

            Value::Undefined => {
                panic!("Undefined value cannot be converted to JSON");
            },

            Value::Raw(s) => {
                s.clone()
            },
        }
    }
}

impl From<&serde_json::Value> for Value {
    fn from(value: &serde_json::Value) -> Self {
        match value {
            serde_json::Value::Null => {
                Value::Null
            },

            serde_json::Value::Bool(b) => {
                Value::Bool(*b)
            },

            serde_json::Value::Number(n) => {
                Value::Number(n.to_string())
            },

            serde_json::Value::String(s) => {
                Value::String(s.to_string())
            },

            serde_json::Value::Array(arr) => {
                Value::Array(arr.iter().map(From::from).collect())
            },

            serde_json::Value::Object(obj) => {
                Value::Object(obj.iter().map(|(k, v)| (k.to_string(), From::from(v))).collect())
            },
        }
    }
}

#[cfg(not(target_pointer_width = "32"))]
impl From<&sonic_rs::Value> for Value {
    fn from(value: &sonic_rs::Value) -> Self {
        match value.as_ref() {
            sonic_rs::ValueRef::Null => {
                Value::Null
            },

            sonic_rs::ValueRef::Bool(b) => {
                Value::Bool(b)
            },

            sonic_rs::ValueRef::Number(n) => {
                Value::Number(n.to_string())
            },

            sonic_rs::ValueRef::String(s) => {
                Value::String(s.to_string())
            },

            sonic_rs::ValueRef::Array(arr) => {
                Value::Array(arr.iter().map(From::from).collect())
            },

            sonic_rs::ValueRef::Object(obj) => {
                Value::Object(obj.iter().map(|(k, v)| (k.to_string(), From::from(v))).collect())
            },
        }
    }
}
