use crate::{node::Field, ops::Ops, yaml_formatter::YamlFormatter, yaml_parser::YamlParser, Error, Formatter, Parser, Path, Value};

pub struct Yaml;

impl Yaml {
    pub fn update_document_field(document: &str, path: Path, value: Value) -> Result<String, Error> {
        let fields
            = Yaml::parse(document)?;

        let mut ops
            = Ops::new();

        ops.set(path, value);

        let result = ops
            .derive::<Yaml>(&fields)
            .apply_to_document(document);

        Ok(result)
    }
}

impl Parser for Yaml {
    fn parse(input: &str) -> Result<Vec<Field>, Error> {
        YamlParser::parse(input)
    }
}

impl Formatter for Yaml {
    fn value_to_string(value: &Value, indent_size: usize, indent: usize) -> String {
        YamlFormatter::value_to_string(value, indent_size, indent)
    }
}
