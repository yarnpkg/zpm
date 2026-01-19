use std::collections::BTreeMap;

use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::{document::Document, Error, JsonDocument, Path, Value, YamlDocument};

/// Determines the document format based on the first non-whitespace character.
/// Returns true for JSON (`{` or `[`), false for YAML (everything else including empty).
fn is_json(input: &[u8]) -> bool {
    let first_byte
        = input.iter()
            .find(|&&b| !b.is_ascii_whitespace())
            .copied();

    matches!(first_byte, Some(b'{') | Some(b'['))
}

/// A document abstraction that automatically detects whether the input is
/// JSON or YAML based on the first non-whitespace character.
///
/// - If the first character is `{` or `[`, it's treated as JSON
/// - Otherwise (including empty string), it's treated as YAML
pub enum DataDocument {
    Json(JsonDocument),
    Yaml(YamlDocument),
}

impl Document for DataDocument {
    fn update_path(&mut self, path: &Path, value: Value) -> Result<(), Error> {
        match self {
            DataDocument::Json(doc) => doc.update_path(path, value),
            DataDocument::Yaml(doc) => doc.update_path(path, value),
        }
    }

    fn set_path(&mut self, path: &Path, value: Value) -> Result<(), Error> {
        match self {
            DataDocument::Json(doc) => doc.set_path(path, value),
            DataDocument::Yaml(doc) => doc.set_path(path, value),
        }
    }
}

impl DataDocument {
    /// Creates a new DataDocument from input bytes, auto-detecting JSON vs YAML format.
    pub fn new(input: Vec<u8>) -> Result<Self, Error> {
        if is_json(&input) {
            Ok(DataDocument::Json(JsonDocument::new(input)?))
        } else {
            Ok(DataDocument::Yaml(YamlDocument::new(input)?))
        }
    }

    /// Returns a reference to the underlying input bytes.
    pub fn input(&self) -> &[u8] {
        match self {
            DataDocument::Json(doc) => &doc.input,
            DataDocument::Yaml(doc) => &doc.input,
        }
    }

    /// Returns a mutable reference to the underlying input bytes.
    pub fn input_mut(&mut self) -> &mut Vec<u8> {
        match self {
            DataDocument::Json(doc) => &mut doc.input,
            DataDocument::Yaml(doc) => &mut doc.input,
        }
    }

    /// Returns a reference to the paths map.
    pub fn paths(&self) -> &BTreeMap<Path, usize> {
        match self {
            DataDocument::Json(doc) => &doc.paths,
            DataDocument::Yaml(doc) => &doc.paths,
        }
    }

    /// Returns whether the document has been changed.
    pub fn changed(&self) -> bool {
        match self {
            DataDocument::Json(doc) => doc.changed,
            DataDocument::Yaml(doc) => doc.changed,
        }
    }

    /// Deserializes a value from a string, auto-detecting JSON vs YAML format.
    pub fn hydrate_from_str<'de, T: Deserialize<'de>>(input: &'de str) -> Result<T, Error> {
        if is_json(input.as_bytes()) {
            JsonDocument::hydrate_from_str(input)
        } else {
            YamlDocument::hydrate_from_str(input)
        }
    }

    /// Deserializes a value from a byte slice, auto-detecting JSON vs YAML format.
    pub fn hydrate_from_slice<'de, T: Deserialize<'de>>(input: &'de [u8]) -> Result<T, Error> {
        if is_json(input) {
            JsonDocument::hydrate_from_slice(input)
        } else {
            YamlDocument::hydrate_from_slice(input)
        }
    }

    /// Serializes a value to a JSON string.
    ///
    /// Note: For serialization, JSON is used by default as it's more widely compatible.
    /// Use `to_yaml_string` if YAML output is specifically needed.
    pub fn to_string<T: Serialize + ?Sized>(input: &T) -> Result<String, Error> {
        JsonDocument::to_string(input)
    }

    /// Serializes a value to a pretty-printed JSON string.
    pub fn to_string_pretty<T: Serialize>(input: &T) -> Result<String, Error> {
        JsonDocument::to_string_pretty(input)
    }

    /// Serializes a value to a YAML string.
    pub fn to_yaml_string<T: Serialize + ?Sized>(input: &T) -> Result<String, Error> {
        YamlDocument::to_string(input)
    }

    /// Sorts the keys of an object at the given path alphabetically.
    pub fn sort_object_keys(&mut self, parent_path: &Path) -> Result<bool, Error> {
        match self {
            DataDocument::Json(doc) => doc.sort_object_keys(parent_path),
            DataDocument::Yaml(doc) => doc.sort_object_keys(parent_path),
        }
    }

    /// Updates a single field in a document string, auto-detecting JSON vs YAML format.
    ///
    /// This is a convenience method that creates a DataDocument, updates the path,
    /// and returns the modified document as a string.
    pub fn update_document_field(document: &str, path: Path, value: Value) -> Result<String, Error> {
        let mut doc
            = DataDocument::new(document.as_bytes().to_vec())?;

        doc.set_path(&path, value)?;

        Ok(String::from_utf8(doc.input().to_vec())
            .map_err(|e| Error::InvalidSyntax(e.to_string()))?)
    }
}

/// A source wrapper that auto-detects JSON vs YAML when parsing from string.
#[derive(Debug)]
pub struct DataSource<T> {
    pub value: T,
}

impl<T: DeserializeOwned> std::str::FromStr for DataSource<T> {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self { value: DataDocument::hydrate_from_str(s)? })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Document;
    use serde::Deserialize;

    #[derive(Debug, Deserialize, PartialEq)]
    struct TestStruct {
        name: String,
        value: i32,
    }

    #[test]
    fn test_json_object_detection() {
        let json_input
            = r#"{"name": "test", "value": 42}"#;

        let result: TestStruct
            = DataDocument::hydrate_from_str(json_input).unwrap();

        assert_eq!(result.name, "test");
        assert_eq!(result.value, 42);
    }

    #[test]
    fn test_json_array_detection() {
        let json_input
            = r#"[1, 2, 3]"#;

        let result: Vec<i32>
            = DataDocument::hydrate_from_str(json_input).unwrap();

        assert_eq!(result, vec![1, 2, 3]);
    }

    #[test]
    fn test_json_with_leading_whitespace() {
        let json_input
            = "   \n\t  {\"name\": \"test\", \"value\": 42}";

        let result: TestStruct
            = DataDocument::hydrate_from_str(json_input).unwrap();

        assert_eq!(result.name, "test");
        assert_eq!(result.value, 42);
    }

    #[test]
    fn test_yaml_object_detection() {
        let yaml_input
            = "name: test\nvalue: 42";

        let result: TestStruct
            = DataDocument::hydrate_from_str(yaml_input).unwrap();

        assert_eq!(result.name, "test");
        assert_eq!(result.value, 42);
    }

    #[test]
    fn test_yaml_empty_string() {
        let yaml_input
            = "";

        let result: Option<String>
            = DataDocument::hydrate_from_str(yaml_input).unwrap();

        assert_eq!(result, None);
    }

    #[test]
    fn test_yaml_with_leading_whitespace() {
        let yaml_input
            = "   \n  name: test\n  value: 42";

        let result: TestStruct
            = DataDocument::hydrate_from_str(yaml_input).unwrap();

        assert_eq!(result.name, "test");
        assert_eq!(result.value, 42);
    }

    #[test]
    fn test_slice_json_detection() {
        let json_input
            = b"{\"name\": \"test\", \"value\": 42}";

        let result: TestStruct
            = DataDocument::hydrate_from_slice(json_input).unwrap();

        assert_eq!(result.name, "test");
        assert_eq!(result.value, 42);
    }

    #[test]
    fn test_slice_yaml_detection() {
        let yaml_input
            = b"name: test\nvalue: 42";

        let result: TestStruct
            = DataDocument::hydrate_from_slice(yaml_input).unwrap();

        assert_eq!(result.name, "test");
        assert_eq!(result.value, 42);
    }

    #[test]
    fn test_json_document_new() {
        let json_input
            = b"{\"test\": \"value\"}".to_vec();

        let doc
            = DataDocument::new(json_input).unwrap();

        assert!(matches!(doc, DataDocument::Json(_)));
        assert!(doc.paths().contains_key(&Path::from_segments(vec!["test".to_string()])));
    }

    #[test]
    fn test_yaml_document_new() {
        let yaml_input
            = b"test: value\n".to_vec();

        let doc
            = DataDocument::new(yaml_input).unwrap();

        assert!(matches!(doc, DataDocument::Yaml(_)));
        assert!(doc.paths().contains_key(&Path::from_segments(vec!["test".to_string()])));
    }

    #[test]
    fn test_json_document_set_path() {
        let json_input
            = b"{\"test\": \"old\"}".to_vec();

        let mut doc
            = DataDocument::new(json_input).unwrap();

        doc.set_path(
            &Path::from_segments(vec!["test".to_string()]),
            Value::String("new".to_string()),
        ).unwrap();

        assert_eq!(
            String::from_utf8(doc.input().to_vec()).unwrap(),
            "{\"test\": \"new\"}"
        );
    }

    #[test]
    fn test_yaml_document_set_path() {
        let yaml_input
            = b"test: old\n".to_vec();

        let mut doc
            = DataDocument::new(yaml_input).unwrap();

        doc.set_path(
            &Path::from_segments(vec!["test".to_string()]),
            Value::String("new".to_string()),
        ).unwrap();

        assert_eq!(
            String::from_utf8(doc.input().to_vec()).unwrap(),
            "test: new\n"
        );
    }
}
