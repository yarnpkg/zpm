#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Number(String), // Store as string to preserve exact formatting
    String(String),
    Array(Vec<Value>),
    Object(Vec<(String, Value)>), // Preserves insertion order
    Undefined, // Used to remove values
}

impl Value {
    pub fn to_json_string(&self, indent_size: usize, indent: usize) -> String {
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
                sonic_rs::to_string(s).expect("Failed to convert string to JSON")
            },

            Value::Array(arr) => {
                let mut serializer
                    = String::new();

                serializer.push_str("[\n");

                let next_indent
                    = indent + indent_size;

                for (i, item) in arr.iter().enumerate() {
                    serializer.push_str(&" ".repeat(next_indent));
                    serializer.push_str(&item.to_json_string(indent_size, next_indent));

                    if i < arr.len() - 1 {
                        serializer.push_str(",\n");
                    } else {
                        serializer.push('\n');
                    }
                }

                serializer.push_str(&" ".repeat(indent));
                serializer.push(']');

                serializer
            },

            Value::Object(obj) => {
                let mut serializer
                    = String::new();

                serializer.push_str("{\n");

                let next_indent
                    = indent + indent_size;

                for (i, (k, v)) in obj.iter().enumerate() {
                    serializer.push_str(&" ".repeat(next_indent));
                    serializer.push_str(&sonic_rs::to_string(k).expect("Failed to convert key to JSON"));
                    serializer.push_str(": ");
                    serializer.push_str(&v.to_json_string(indent_size, next_indent));

                    if i < obj.len() - 1 {
                        serializer.push_str(",\n");
                    } else {
                        serializer.push('\n');
                    }
                }

                serializer.push_str(&" ".repeat(indent));
                serializer.push('}');

                serializer
            },

            Value::Undefined => {
                panic!("Undefined value cannot be converted to JSON");
            },
        }
    }
}

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

impl From<serde_json::Value> for Value {
    fn from(value: serde_json::Value) -> Self {
        match value {
            serde_json::Value::Null => {
                Value::Null
            },

            serde_json::Value::Bool(b) => {
                Value::Bool(b)
            },

            serde_json::Value::Number(n) => {
                Value::Number(n.to_string())
            },

            serde_json::Value::String(s) => {
                Value::String(s.clone())
            },

            serde_json::Value::Array(arr) => {
                Value::Array(arr.into_iter().map(From::from).collect())
            },

            serde_json::Value::Object(obj) => {
                Value::Object(obj.into_iter().map(|(k, v)| {
                    (k, From::from(v))
                }).collect())
            },
        }
    }
}
