use ouroboros::self_referencing;
use zpm_utils::Path;
use colored::Colorize;
use serde::{de, Deserialize, Deserializer, Serialize};
use zpm_utils::{FromFileString, ToFileString, ToHumanString};
use std::collections::BTreeMap;

use crate::{config::{Password, SettingSource, CONFIG_PATH}, error::Error};

pub type StringField = StringLikeField<String>;
pub type GlobField = StringLikeField<Glob>;

pub trait Field<T> {
    fn value(&self) -> &T;
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
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
pub struct OptionalStringField {
    pub value: Option<String>,
    pub source: SettingSource,
}

impl OptionalStringField {
    pub fn new(value: Option<String>) -> Self {
        Self {value, source: Default::default()}
    }
}

impl FromFileString for OptionalStringField {
    type Error = Error;

    fn from_file_string(raw: &str) -> Result<Self, Self::Error> {
        Ok(Self {value: Some(raw.to_string()), source: Default::default()})
    }
}

impl ToFileString for OptionalStringField {
    fn to_file_string(&self) -> String {
        self.value.as_ref().unwrap_or(&"".to_string()).to_string()
    }
}

impl ToHumanString for OptionalStringField {
    fn to_print_string(&self) -> String {
        self.value.as_ref().unwrap_or(&"".to_string()).to_string()
    }
}

impl<'de> Deserialize<'de> for OptionalStringField {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> where D: Deserializer<'de> {
        Ok(Self {value: Option::<String>::deserialize(deserializer)?, source: Default::default()})
    }
}

impl Serialize for OptionalStringField {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where S: serde::Serializer {
        self.value.serialize(serializer)
    }
}

#[derive(Debug, Clone)]
pub struct OptionalPasswordField {
    pub value: Option<Password>,
    pub source: SettingSource,
}

impl OptionalPasswordField {
    pub fn new(value: Option<Password>) -> Self {
        Self {value, source: Default::default()}
    }
}

impl FromFileString for OptionalPasswordField {
    type Error = Error;

    fn from_file_string(raw: &str) -> Result<Self, Self::Error> {
        Ok(Self {value: Some(Password {value: raw.to_string()}), source: Default::default()})
    }
}

impl ToFileString for OptionalPasswordField {
    fn to_file_string(&self) -> String {
        self.value.as_ref().unwrap_or(&Password {value: "".to_string()}).value.clone()
    }
}

impl ToHumanString for OptionalPasswordField {
    fn to_print_string(&self) -> String {
        self.value.as_ref().unwrap_or(&Password {value: "".to_string()}).value.clone()
    }
}

impl<'de> Deserialize<'de> for OptionalPasswordField {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> where D: Deserializer<'de> {
        Ok(Self {value: Option::<Password>::deserialize(deserializer)?, source: Default::default()})
    }
}

impl Serialize for OptionalPasswordField {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where S: serde::Serializer {
        self.value.serialize(serializer)
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
        self.value.to_file_string()
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
        self.value.to_file_string().serialize(serializer)
    }
}

#[self_referencing]
#[derive(Debug)]
struct OwnedGlob {
    raw: String,

    #[borrows(raw)]
    #[covariant]
    pattern: wax::Glob<'this>,
}

impl PartialEq for OwnedGlob {
    fn eq(&self, other: &Self) -> bool {
        self.borrow_raw() == other.borrow_raw()
    }
}

impl Eq for OwnedGlob {}

impl PartialOrd for OwnedGlob {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.borrow_raw().partial_cmp(other.borrow_raw())
    }
}

impl Ord for OwnedGlob {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.borrow_raw().cmp(other.borrow_raw())
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Glob {
    inner: OwnedGlob,
}

impl Clone for Glob {
    fn clone(&self) -> Self {
        Self::parse(self.inner.borrow_raw().clone()).unwrap()
    }
}

impl Glob {
    pub fn parse(raw: impl Into<String>) -> Result<Self, Error> {
        let raw = raw.into();

        let pattern = OwnedGlobTryBuilder {
            raw,
            pattern_builder: |raw| wax::Glob::new(raw).map_err(|_| Error::InvalidGlob(raw.clone())),
        }.try_build()?;

        Ok(Glob { inner: pattern })
    }

    pub fn raw(&self) -> &str {
        self.inner.borrow_raw()
    }

    pub fn matcher(&self) -> &wax::Glob {
        self.inner.borrow_pattern()
    }

    pub fn to_regex_string(&self) -> String {
        self.matcher()
            .to_regex()
            .to_string()
    }
}

impl ToFileString for Glob {
    fn to_file_string(&self) -> String {
        self.inner.borrow_raw().clone()
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
        Ok(Glob::parse(raw)?)
    }
}

impl<'de> Deserialize<'de> for Glob {
    fn deserialize<D>(deserializer: D) -> Result<Glob, D::Error> where D: Deserializer<'de> {
        Ok(Glob::parse(String::deserialize(deserializer)?)
            .map_err(|err| de::Error::custom(err.to_string()))?)
    }
}

impl Serialize for Glob {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where S: serde::Serializer {
        self.inner.borrow_raw().serialize(serializer)
    }
}

#[derive(Debug, Clone)]
pub struct DictField<K, V> {
    pub value: BTreeMap<K, V>,
    pub source: SettingSource,
}

impl<K, V> DictField<K, V> {
    pub fn new(value: BTreeMap<K, V>) -> Self {
        Self {value, source: Default::default()}
    }
}

impl<K, V> Field<BTreeMap<K, V>> for DictField<K, V> {
    fn value(&self) -> &BTreeMap<K, V> {
        &self.value
    }
}

impl<K: Serialize + Ord, V: Serialize> ToFileString for DictField<K, V> {
    fn to_file_string(&self) -> String {
        sonic_rs::to_string(&self.value).unwrap()
    }
}

impl<K: ToHumanString + Ord, V: ToHumanString> ToHumanString for DictField<K, V> {
    fn to_print_string(&self) -> String {
        let entries: Vec<String> = self.value.iter()
            .map(|(k, v)| format!("{}: {}", k.to_print_string(), v.to_print_string()))
            .collect();
        format!("{{{}}}", entries.join(", "))
    }
}

impl<K: FromFileString + Ord, V: FromFileString + for<'a> Deserialize<'a>> FromFileString for DictField<K, V>
where
    K: for<'a> Deserialize<'a>,
    Error: From<<K as FromFileString>::Error> + From<<V as FromFileString>::Error>,
{
    type Error = sonic_rs::Error;

    fn from_file_string(raw: &str) -> Result<Self, Self::Error> {
        // If the string starts with '{', it's a JSON object
        if raw.starts_with('{') {
            let value = sonic_rs::from_str::<BTreeMap<K, V>>(raw)?;
            Ok(Self {value, source: Default::default()})
        } else {
            // Otherwise, treat it as a single key:value pair
            // This allows for simpler syntax in config files
            let parts: Vec<&str> = raw.splitn(2, ':').collect();
            if parts.len() != 2 {
                return Err(serde::de::Error::custom("Expected key:value format"));
            }

            let key = K::from_file_string(parts[0])
                .map_err(|_| serde::de::Error::custom("Failed to parse key"))?;
            let val = V::from_file_string(parts[1].trim())
                .map_err(|_| serde::de::Error::custom("Failed to parse value"))?;

            let mut map = BTreeMap::new();
            map.insert(key, val);
            Ok(Self {value: map, source: Default::default()})
        }
    }
}

impl<'de, K, V> Deserialize<'de> for DictField<K, V>
where
    K: Deserialize<'de> + Ord,
    V: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> where D: Deserializer<'de> {
        Ok(Self {value: BTreeMap::<K, V>::deserialize(deserializer)?, source: Default::default()})
    }
}

impl<K: Serialize, V: Serialize> Serialize for DictField<K, V> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where S: serde::Serializer {
        self.value.serialize(serializer)
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
        assert_eq!(glob_field.value.raw(), "*.rs");

        // Test ToFileString
        assert_eq!(glob_field.to_file_string(), "*.rs");

        // Test Serialize/Deserialize
        let serialized = serde_json::to_string(&glob_field).unwrap();
        assert_eq!(serialized, "\"*.rs\"");
        let deserialized: GlobField = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized.value.raw(), "*.rs");
    }

    #[test]
    fn test_glob() {
        // Test FromFileString
        let raw = "*.txt";
        let glob = Glob::from_file_string(raw).unwrap();
        assert_eq!(glob.raw(), "*.txt");

        // Test ToFileString
        assert_eq!(glob.to_file_string(), "*.txt");

        // Test Serialize/Deserialize
        let serialized = serde_json::to_string(&glob).unwrap();
        assert_eq!(serialized, "\"*.txt\"");
        let deserialized: Glob = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized.raw(), "*.txt");

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
        assert_eq!(path_field_absolute.value.to_file_string(), "/absolute/path/to/file.txt");

        // Test FromFileString with relative path
        let raw_relative = "relative/path/to/file.txt";
        let path_field_relative = PathField::from_file_string(raw_relative).unwrap();
        assert_eq!(path_field_relative.value.to_file_string(), "/tmp/relative/path/to/file.txt");

        // Test ToFileString
        assert_eq!(path_field_absolute.to_file_string(), "/absolute/path/to/file.txt");

        // Test Serialize
        let serialized = serde_json::to_string(&path_field_absolute).unwrap();
        assert_eq!(serialized, "\"/absolute/path/to/file.txt\"");

        // Test Deserialize with absolute path
        let json_str = "\"/absolute/path/to/file.txt\"";
        let deserialized: PathField = serde_json::from_str(json_str).unwrap();
        assert_eq!(deserialized.value.to_file_string(), "/absolute/path/to/file.txt");

        // Test Deserialize with relative path
        let json_str = "\"relative/path/to/file.txt\"";
        let deserialized: PathField = serde_json::from_str(json_str).unwrap();
        assert_eq!(deserialized.value.to_file_string(), "/tmp/relative/path/to/file.txt");
    }

    #[test]
    fn test_dict_field() {
        // Test FromFileString (single key:value)
        let dict_field = DictField::<String, String>::from_file_string("key1:value1").unwrap();
        assert_eq!(dict_field.value.len(), 1);
        assert_eq!(dict_field.value.get("key1"), Some(&"value1".to_string()));

        // Test FromFileString (JSON object)
        let dict_field = DictField::<String, String>::from_file_string("{\"key1\":\"value1\",\"key2\":\"value2\"}").unwrap();
        assert_eq!(dict_field.value.len(), 2);
        assert_eq!(dict_field.value.get("key1"), Some(&"value1".to_string()));
        assert_eq!(dict_field.value.get("key2"), Some(&"value2".to_string()));

        // Test ToFileString
        let mut map = BTreeMap::new();
        map.insert("a".to_string(), "1".to_string());
        map.insert("b".to_string(), "2".to_string());
        let dict_field = DictField::new(map);
        let file_string = dict_field.to_file_string();
        assert_eq!(file_string, "{\"a\":\"1\",\"b\":\"2\"}");

        // Test ToHumanString - Note: StringField adds color codes to the output
        let human_string = dict_field.to_print_string();
        // The exact format will include ANSI color codes from StringField's to_print_string
        assert!(human_string.contains("a:"));
        assert!(human_string.contains("b:"));
        assert!(human_string.contains("1"));
        assert!(human_string.contains("2"));
        assert!(human_string.starts_with("{"));
        assert!(human_string.ends_with("}"));

        // Test Serialize
        let serialized = serde_json::to_string(&dict_field).unwrap();
        assert_eq!(serialized, "{\"a\":\"1\",\"b\":\"2\"}");

        // Test Deserialize
        let deserialized: DictField<String, String> = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized.value, dict_field.value);

        // Test with different value types
        let mut int_map = BTreeMap::new();
        int_map.insert("count".to_string(), 42u64);
        int_map.insert("total".to_string(), 100u64);
        let int_dict_field = DictField::new(int_map);

        let serialized = serde_json::to_string(&int_dict_field).unwrap();
        assert_eq!(serialized, "{\"count\":42,\"total\":100}");
    }
}
