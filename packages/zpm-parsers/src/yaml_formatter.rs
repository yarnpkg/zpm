use crate::{Formatter, JsonDocument, Value};

pub struct YamlFormatter;

impl Formatter for YamlFormatter {
    fn value_to_string(value: &Value, indent_size: usize, indent: usize) -> String {
        match value {
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
                if s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
                    return s.to_string();
                }

                JsonDocument::to_string(s).expect("Failed to convert string to JSON")
            },

            Value::Array(arr) => {
                if arr.is_empty() {
                    return "[]".to_string();
                }

                let mut serializer
                    = String::new();

                serializer.push_str("\n");

                let next_indent
                    = indent + indent_size;

                for item in arr.iter() {
                    serializer.push_str(&" ".repeat(next_indent));
                    serializer.push_str("- ");
                    serializer.push_str(&YamlFormatter::value_to_string(item, indent_size, next_indent));
                    serializer.push('\n');
                }

                // Remove the last newline
                serializer.pop();

                serializer
            },

            Value::Object(obj) => {
                let mut serializer
                    = String::new();

                serializer.push_str("\n");

                let next_indent
                    = indent + indent_size;

                for item in obj.iter() {
                    serializer.push_str(&" ".repeat(indent));
                    serializer.push_str(&YamlFormatter::value_to_string(&Value::String(item.0.clone()), indent_size, next_indent));
                    serializer.push(':');

                    let serialized_value
                        = YamlFormatter::value_to_string(&item.1, indent_size, next_indent);

                    if !serialized_value.starts_with('\n') {
                        serializer.push(' ');
                    }

                    serializer.push_str(&serialized_value);
                    serializer.push('\n');
                }

                // Remove the last newline
                serializer.pop();

                serializer
            },

            Value::Undefined => {
                panic!("Undefined value cannot be converted to YAML");
            },

            Value::Raw(s) => {
                s.clone()
            },
        }
    }
}
