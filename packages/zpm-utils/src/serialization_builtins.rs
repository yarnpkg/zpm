use std::fmt::{self, Display};

use crate::{DataType, ToHumanString};

pub struct ColoredJsonValue(serde_json::Value);

impl<'de> serde::Deserialize<'de> for ColoredJsonValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        Ok(ColoredJsonValue(value))
    }
}

impl ToHumanString for ColoredJsonValue {
    fn to_print_string(&self) -> String {
        self.0.to_print_string()
    }
}

impl std::fmt::Debug for ColoredJsonValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

impl Display for ColoredJsonValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_print_string())
    }
}

impl ToHumanString for serde_json::Value {
    fn to_print_string(&self) -> String {
        match self {
            serde_json::Value::String(s) => {
                let serialized
                    = serde_json::to_string(s).unwrap();

                DataType::String.colorize(&serialized)
            },

            serde_json::Value::Number(n) => {
                DataType::Number.colorize(&n.to_string())
            },

            serde_json::Value::Bool(b) => {
                DataType::Boolean.colorize(&b.to_string())
            },

            serde_json::Value::Null => {
                DataType::Null.colorize("null")
            },

            serde_json::Value::Array(a) => {
                let left = DataType::Code.colorize("[ ");
                let join = DataType::Code.colorize(", ");
                let right = DataType::Code.colorize(" ]");

                let mut result = String::new();

                result.push_str(&left);
                result.push_str(&a.iter().map(|v| v.to_print_string()).collect::<Vec<_>>().join(&join));
                result.push_str(&right);

                result
            },

            serde_json::Value::Object(o) => {
                let left = DataType::Code.colorize("{ ");
                let join = DataType::Code.colorize(", ");
                let colon = DataType::Code.colorize(": ");
                let right = DataType::Code.colorize(" }");

                let mapped = o.iter().map(|(k, v)| {
                    let serialized_key
                        = serde_json::to_string(k).unwrap();

                    let mut result = String::new();

                    result.push_str(&DataType::String.colorize(&serialized_key));
                    result.push_str(&colon);
                    result.push_str(&v.to_print_string());

                    result
                }).collect::<Vec<_>>();

                let mut result = String::new();

                result.push_str(&left);
                result.push_str(&mapped.join(&join));
                result.push_str(&right);

                result
            },
        }
    }
}
