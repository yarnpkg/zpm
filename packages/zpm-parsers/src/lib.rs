pub mod node;
pub mod ops;
pub mod value;
pub mod yaml_formatter;
pub mod yaml_parser;
pub mod yaml;

pub mod error;
pub mod path;
pub mod json;

pub use error::Error;
pub use path::Path;
pub use json::JsonFormatter;
pub use value::Value;

use crate::node::Field;

pub trait Parser {
    fn parse(input: &str) -> Result<Vec<Field>, Error>;
}

pub trait Formatter {
    fn value_to_string(value: &Value, indent_size: usize, indent: usize) -> String;
}
