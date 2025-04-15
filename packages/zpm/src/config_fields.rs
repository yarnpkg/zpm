use zpm_utils::Path;
use colored::Colorize;
use serde::{de, Deserialize, Deserializer, Serialize};
use zpm_utils::{FromFileString, ToFileString, ToHumanString};

use crate::{config::{SettingSource, CONFIG_PATH}, error::Error};

pub type StringField = StringLikeField<String>;
pub type GlobField = StringLikeField<Glob>;

pub trait Field<T> {
    fn value(&self) -> &T;
}

#[derive(Debug, Clone)]
pub struct StringLikeField<T> {
    pub value: T,
    pub source: SettingSource,
}

impl<T> StringLikeField<T> {
    pub fn new(value: T) -> Self {
        Self {value, source: Default::default()}
    }
}

impl<T> Field<T> for StringLikeField<T> {
    fn value(&self) -> &T {
        &self.value
    }
}

impl<T: ToFileString> ToFileString for StringLikeField<T> {
    fn to_file_string(&self) -> String {
        self.value.to_file_string()
    }
}

impl<T: ToHumanString> ToHumanString for StringLikeField<T> {
    fn to_print_string(&self) -> String {
        self.value.to_print_string()
    }
}

impl<T: FromFileString> FromFileString for StringLikeField<T> where Error: From<<T as FromFileString>::Error> {
    type Error = Error;

    fn from_file_string(raw: &str) -> Result<Self, Self::Error> {
        Ok(Self {value: T::from_file_string(raw)?, source: Default::default()})
    }
}

impl<T: Serialize> Serialize for StringLikeField<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where S: serde::Serializer {
        self.value.serialize(serializer)
    }
}

impl<'de, T> Deserialize<'de> for StringLikeField<T> where T: Deserialize<'de> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> where D: Deserializer<'de> {
        Ok(Self {value: T::deserialize(deserializer)?, source: Default::default()})
    }
}

#[derive(Debug, Clone)]
pub struct BoolField {
    pub value: bool,
    pub source: SettingSource,
}

impl BoolField {
    pub fn new(value: bool) -> Self {
        Self {value, source: Default::default()}
    }
}

impl Field<bool> for BoolField {
    fn value(&self) -> &bool {
        &self.value
    }
}

impl FromFileString for BoolField {
    type Error = Error;

    fn from_file_string(raw: &str) -> Result<Self, Self::Error> {
        let value = match raw {
            "true" | "1" => true,
            "false" | "0" => false,
            _ => panic!("Invalid boolean value"),
        };

        Ok(BoolField {value, source: Default::default()})
    }
}

impl ToFileString for BoolField {
    fn to_file_string(&self) -> String {
        self.value.to_string()
    }
}

impl ToHumanString for BoolField {
    fn to_print_string(&self) -> String {
        self.to_file_string().truecolor(255, 153, 0).to_string()
    }
}

impl<'de> Deserialize<'de> for BoolField {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> where D: Deserializer<'de> {
        Ok(Self {value: bool::deserialize(deserializer)?, source: Default::default()})
    }
}

impl Serialize for BoolField {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where S: serde::Serializer {
        self.value.serialize(serializer)
    }
}

#[derive(Debug, Clone)]
pub struct UintField {
    pub value: u64,
    pub source: SettingSource,
}

impl UintField {
    pub fn new(value: u64) -> Self {
        Self {value, source: Default::default()}
    }
}

impl Field<u64> for UintField {
    fn value(&self) -> &u64 {
        &self.value
    }
}

impl ToFileString for UintField {
    fn to_file_string(&self) -> String {
        self.value.to_string()
    }
}

impl ToHumanString for UintField {
    fn to_print_string(&self) -> String {
        self.to_file_string().truecolor(255, 255, 0).to_string()
    }
}

impl FromFileString for UintField {
    type Error = Error;

    fn from_file_string(raw: &str) -> Result<Self, Self::Error> {
        Ok(UintField {value: raw.parse()?, source: Default::default()})
    }
}

impl<'de> Deserialize<'de> for UintField {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> where D: Deserializer<'de> {
        Ok(Self {value: u64::deserialize(deserializer)?, source: Default::default()})
    }
}

impl Serialize for UintField {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where S: serde::Serializer {
        self.value.serialize(serializer)
    }
}

#[derive(Debug, Clone)]
pub struct JsonField<T> {
    pub value: T,
    pub source: SettingSource,
}

impl<T> JsonField<T> {
    pub fn new(value: T) -> Self {
        Self {value, source: SettingSource::Default}
    }
}

impl<T: Field<T>> Field<T> for JsonField<T> {
    fn value(&self) -> &T {
        &self.value
    }
}

impl<T: ToFileString> ToFileString for JsonField<T> {
    fn to_file_string(&self) -> String {
        self.value.to_file_string()
    }
}

impl<T: ToHumanString> ToHumanString for JsonField<T> {
    fn to_print_string(&self) -> String {
        self.value.to_print_string()
    }
}

impl<'de, T> Deserialize<'de> for JsonField<T> where T: Deserialize<'de> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> where D: Deserializer<'de> {
        Ok(Self {value: T::deserialize(deserializer)?, source: Default::default()})
    }
}

impl<T: Serialize> Serialize for JsonField<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where S: serde::Serializer {
        self.value.serialize(serializer)
    }
}

#[derive(Debug, Clone)]
pub struct VecField<T> {
    pub value: Vec<T>,
}

impl<T> VecField<T> {
    pub fn new(value: Vec<T>) -> Self {
        Self {value}
    }
}

impl<T: Field<T>> Field<Vec<T>> for VecField<T> {
    fn value(&self) -> &Vec<T> {
        &self.value
    }
}

impl<T: Serialize> ToFileString for VecField<T> {
    fn to_file_string(&self) -> String {
        sonic_rs::to_string(&self.value).unwrap()
    }
}

impl<T: ToHumanString> ToHumanString for VecField<T> {
    fn to_print_string(&self) -> String {
        format!("[{}]", self.value.iter().map(|v| v.to_print_string()).collect::<Vec<_>>().join(", "))
    }
}

impl<T: FromFileString + for<'a> Deserialize<'a>> FromFileString for VecField<T> {
    type Error = sonic_rs::Error;

    fn from_file_string(raw: &str) -> Result<Self, Self::Error> {
        if raw.starts_with('[') {
            let value = sonic_rs::from_str::<Vec<T>>(raw)?;

            Ok(Self {value})  
        } else {
            let value = T::from_file_string(raw)
                .map_err(|_| serde::de::Error::custom("Failed to call FromFileString"))?;

            Ok(Self {value: vec![value]})
        }
    }
}

impl<'de, T> Deserialize<'de> for VecField<T> where T: Deserialize<'de> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> where D: Deserializer<'de> {
        Ok(VecField {value: Vec::<T>::deserialize(deserializer)?})
    }
}

impl<T: Serialize> Serialize for VecField<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where S: serde::Serializer {
        self.value.serialize(serializer)
    }
}

#[derive(Debug, Clone)]
pub struct EnumField<T> {
    pub value: T,
    pub source: SettingSource,
}

impl<T> EnumField<T> {
    pub fn new(value: T) -> Self {
        Self {value, source: Default::default()}
    }
}

impl<T: Field<T>> Field<T> for EnumField<T> {
    fn value(&self) -> &T {
        &self.value
    }
}

impl<T: Serialize> ToFileString for EnumField<T> {
    fn to_file_string(&self) -> String {
        serde_plain::to_string(&self.value).unwrap()
    }
}

impl<T: Serialize> ToHumanString for EnumField<T> {
    fn to_print_string(&self) -> String {
        self.to_file_string()
    }
}

impl<T: for<'de> Deserialize<'de>> FromFileString for EnumField<T> {
    type Error = Error;

    fn from_file_string(raw: &str) -> Result<Self, Self::Error> {
        Ok(EnumField {value: serde_plain::from_str::<T>(raw).map_err(|_| Error::Unsupported)?, source: Default::default()})
    }
}

impl<'de, T: for<'a> Deserialize<'a>> Deserialize<'de> for EnumField<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> where D: Deserializer<'de> {
        Ok(Self {value: T::deserialize(deserializer)?, source: Default::default()})
    }
}

impl<T: Serialize> Serialize for EnumField<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where S: serde::Serializer {
        self.value.serialize(serializer)
    }
}

#[derive(Debug, Clone)]
pub struct PathField {
    pub value: Path,
    pub source: SettingSource,
}

impl PathField {
    pub fn new(value: Path) -> Self {
        Self {value, source: Default::default()}
    }
}

impl Field<Path> for PathField {
    fn value(&self) -> &Path {
        &self.value
    }
}

impl ToFileString for PathField {
    fn to_file_string(&self) -> String {
        self.value.to_string()
    }
}

impl ToHumanString for PathField {
    fn to_print_string(&self) -> String {
        self.to_file_string().truecolor(153, 153, 255).to_string()
    }
}

impl FromFileString for PathField {
    type Error = Error;

    fn from_file_string(raw: &str) -> Result<Self, Self::Error> {
        let mut value = Path::try_from(raw)?;

        if !value.is_absolute() {
            value = CONFIG_PATH.lock().unwrap()
                .as_ref().unwrap()
                .rc_path.as_ref().unwrap()
                .dirname().unwrap()
                .with_join(&value);
        }

        Ok(Self {value, source: Default::default()})
    }
}

impl<'de> Deserialize<'de> for PathField {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> where D: Deserializer<'de> {
        let mut value = Path::try_from(String::deserialize(deserializer)?)
            .map_err(|err| de::Error::custom(err.to_string()))?;

        if !value.is_absolute() {
            value = CONFIG_PATH.lock().unwrap()
                .as_ref().unwrap()
                .rc_path.as_ref().unwrap()
                .dirname().unwrap()
                .with_join(&value);
        }

        Ok(PathField {value, source: Default::default()})
    }
}

impl Serialize for PathField {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where S: serde::Serializer {
        self.value.to_string().serialize(serializer)
    }
}

#[derive(Debug, Clone)]
pub struct Glob {
    pub pattern: String,
}

impl Glob {
    pub fn to_regex_string(&self) -> String {
        wax::Glob::new(&self.pattern)
            .unwrap()
            .to_regex()
            .to_string()
    }
}

impl ToFileString for Glob {
    fn to_file_string(&self) -> String {
        self.pattern.clone()
    }
}

impl ToHumanString for Glob {
    fn to_print_string(&self) -> String {
        self.to_file_string().truecolor(153, 153, 255).to_string()
    }
}

impl FromFileString for Glob {
    type Error = Error;

    fn from_file_string(raw: &str) -> Result<Self, Self::Error> {
        Ok(Glob {pattern: raw.to_string()})
    }
}

impl<'de> Deserialize<'de> for Glob {
    fn deserialize<D>(deserializer: D) -> Result<Glob, D::Error> where D: Deserializer<'de> {
        Ok(Glob { pattern: String::deserialize(deserializer)? })
    }
}

impl Serialize for Glob {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where S: serde::Serializer {
        self.pattern.serialize(serializer)
    }
}

#[cfg(test)]
mod tests {
    use crate::config::ConfigPaths;

    use super::*;
    use serde::{Deserialize, Serialize};
    use serde_json;

    #[test]
    fn test_string_field() {
        // Test FromFileString
        let raw = "test_string";
        let string_field = StringField::from_file_string(raw).unwrap();
        assert_eq!(string_field.value, "test_string");

        // Test ToFileString
        assert_eq!(string_field.to_file_string(), "test_string");

        // Test Serialize/Deserialize
        let serialized = serde_json::to_string(&string_field).unwrap();
        assert_eq!(serialized, "\"test_string\"");

        let deserialized: StringField = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized.value, string_field.value);
    }

    #[test]
    fn test_bool_field() {
        // Test FromFileString - true
        let raw_true = "true";
        let bool_field_true = BoolField::from_file_string(raw_true).unwrap();
        assert_eq!(bool_field_true.value, true);

        // Test FromFileString - false
        let raw_false = "false";
        let bool_field_false = BoolField::from_file_string(raw_false).unwrap();
        assert_eq!(bool_field_false.value, false);

        // Test alternative representations
        let bool_field_1 = BoolField::from_file_string("1").unwrap();
        assert_eq!(bool_field_1.value, true);
        let bool_field_0 = BoolField::from_file_string("0").unwrap();
        assert_eq!(bool_field_0.value, false);

        // Test ToFileString
        assert_eq!(bool_field_true.to_file_string(), "true");
        assert_eq!(bool_field_false.to_file_string(), "false");

        // Test Serialize/Deserialize
        let serialized_true = serde_json::to_string(&bool_field_true).unwrap();
        assert_eq!(serialized_true, "true");
        let deserialized_true: BoolField = serde_json::from_str(&serialized_true).unwrap();
        assert_eq!(deserialized_true.value, true);
    }

    #[test]
    fn test_uint_field() {
        // Test FromFileString
        let raw = "42";
        let uint_field = UintField::from_file_string(raw).unwrap();
        assert_eq!(uint_field.value, 42);

        // Test ToFileString
        assert_eq!(uint_field.to_file_string(), "42");

        // Test Serialize/Deserialize
        let serialized = serde_json::to_string(&uint_field).unwrap();
        assert_eq!(serialized, "42");
        let deserialized: UintField = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized.value, 42);
    }

    #[test]
    fn test_glob_field() {
        // Test FromFileString
        let raw = "*.rs";
        let glob_field = GlobField::from_file_string(raw).unwrap();
        assert_eq!(glob_field.value.pattern, "*.rs");

        // Test ToFileString
        assert_eq!(glob_field.to_file_string(), "*.rs");

        // Test Serialize/Deserialize
        let serialized = serde_json::to_string(&glob_field).unwrap();
        assert_eq!(serialized, "\"*.rs\"");
        let deserialized: GlobField = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized.value.pattern, "*.rs");
    }

    #[test]
    fn test_glob() {
        // Test FromFileString
        let raw = "*.txt";
        let glob = Glob::from_file_string(raw).unwrap();
        assert_eq!(glob.pattern, "*.txt");

        // Test ToFileString
        assert_eq!(glob.to_file_string(), "*.txt");

        // Test Serialize/Deserialize
        let serialized = serde_json::to_string(&glob).unwrap();
        assert_eq!(serialized, "\"*.txt\"");
        let deserialized: Glob = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized.pattern, "*.txt");

        // Test regex conversion - Check for a pattern that would be in a regex that matches "*.txt"
        let regex_str = glob.to_regex_string();
        assert!(regex_str.contains("\\.txt") || regex_str.contains("\\.(txt)") || regex_str.contains("[.]txt"));
    }

    #[test]
    fn test_json_field() {
        // For JsonField we need a concrete type to test with
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
        struct TestStruct {
            name: String,
            value: i32,
        }

        impl ToFileString for TestStruct {
            fn to_file_string(&self) -> String {
                format!("{}:{}", self.name, self.value)
            }
        }

        impl ToHumanString for TestStruct {
            fn to_print_string(&self) -> String {
                self.to_file_string()
            }
        }

        let test_struct = TestStruct {
            name: "test".to_string(),
            value: 123,
        };

        let json_field = JsonField::new(test_struct.clone());

        // Test ToFileString
        assert_eq!(json_field.to_file_string(), "test:123");

        // Test Serialize/Deserialize
        let serialized = serde_json::to_string(&json_field).unwrap();
        assert_eq!(serialized, "{\"name\":\"test\",\"value\":123}");
        let deserialized: JsonField<TestStruct> = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized.value, test_struct);
    }

    #[test]
    fn test_vec_field() {
        // Test FromFileString (single value)
        let vec_field = VecField::<String>::from_file_string("one").unwrap();
        assert_eq!(vec_field.value, vec!["one".to_string()]);

        // Test FromFileString (multiple values)
        let vec_field = VecField::<String>::from_file_string("[\"one\",\"two\",\"three\"]").unwrap();
        assert_eq!(vec_field.value, vec!["one".to_string(), "two".to_string(), "three".to_string()]);

        // Test ToFileString (single value)
        let file_string = VecField::new(vec!["one".to_string()]).to_file_string();
        assert_eq!(file_string, "[\"one\"]");

        // Test ToFileString (multiple values)
        let file_string = VecField::new(vec!["one".to_string(), "two".to_string(), "three".to_string()]).to_file_string();
        assert_eq!(file_string, "[\"one\",\"two\",\"three\"]");

        // Test Serialize
        let serialized = serde_json::to_string(&vec_field).unwrap();
        assert_eq!(serialized, "[\"one\",\"two\",\"three\"]");

        // Test Deserialize
        let deserialized: VecField<String> = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized.value, vec_field.value);
    }

    #[test]
    fn test_enum_field() {
        // Define a simple enum to test with
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
        enum TestEnum {
            Option1,
            Option2,
            Option3,
        }

        // Test FromFileString
        let enum_field: EnumField<TestEnum> = EnumField::from_file_string("Option1").unwrap();
        assert_eq!(enum_field.value, TestEnum::Option1);

        // Test ToFileString
        assert_eq!(enum_field.to_file_string(), "Option1");

        // Test Serialize
        let serialized = serde_json::to_string(&enum_field).unwrap();
        assert_eq!(serialized, "\"Option1\"");

        // Test Deserialize
        let deserialized: EnumField<TestEnum> = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized.value, enum_field.value);
    }

    #[test]
    fn test_path_field() {
        // We need to set up the CONFIG_PATH for this test
        let temp_path = Path::try_from("/tmp/test_config.toml").unwrap();
        *CONFIG_PATH.lock().unwrap() = Some(ConfigPaths {
            rc_path: Some(temp_path.clone()),
            project_cwd: None,
            package_cwd: None,
        });

        // Test FromFileString with absolute path
        let raw_absolute = "/absolute/path/to/file.txt";
        let path_field_absolute = PathField::from_file_string(raw_absolute).unwrap();
        assert_eq!(path_field_absolute.value.to_string(), "/absolute/path/to/file.txt");

        // Test FromFileString with relative path
        let raw_relative = "relative/path/to/file.txt";
        let path_field_relative = PathField::from_file_string(raw_relative).unwrap();
        assert_eq!(path_field_relative.value.to_string(), "/tmp/relative/path/to/file.txt");

        // Test ToFileString
        assert_eq!(path_field_absolute.to_file_string(), "/absolute/path/to/file.txt");

        // Test Serialize
        let serialized = serde_json::to_string(&path_field_absolute).unwrap();
        assert_eq!(serialized, "\"/absolute/path/to/file.txt\"");
        
        // Test Deserialize with absolute path
        let json_str = "\"/absolute/path/to/file.txt\"";
        let deserialized: PathField = serde_json::from_str(json_str).unwrap();
        assert_eq!(deserialized.value.to_string(), "/absolute/path/to/file.txt");

        // Test Deserialize with relative path
        let json_str = "\"relative/path/to/file.txt\"";
        let deserialized: PathField = serde_json::from_str(json_str).unwrap();
        assert_eq!(deserialized.value.to_string(), "/tmp/relative/path/to/file.txt");
    }
}
