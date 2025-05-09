use std::fmt;

use zpm_utils::{Path, RawPath};
use bincode::{Decode, Encode};
use serde::{de::{self, Visitor}, ser::SerializeMap, Deserialize, Deserializer, Serialize, Serializer};

#[derive(Clone, Debug, Encode, Decode, PartialEq, Eq)]
pub enum ImportsField {
    Path(RawPath),
    Package(String),
    Map(Vec<(String, ImportsField)>),
}

impl<'a> ImportsField {
    pub fn paths(&'a self) -> impl Iterator<Item = &'a RawPath> {
        ImportsFieldPathIter::new(self)
    }
}

impl<'de> Deserialize<'de> for ImportsField {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> where D: Deserializer<'de> {
        deserializer.deserialize_any(ImportsFieldEntriesVisitor {})
    }
}

impl Serialize for ImportsField {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where S: Serializer {
        match self {
            ImportsField::Path(raw_path) => serializer.serialize_str(&raw_path.raw),
            ImportsField::Package(package) => serializer.serialize_str(&package),
            ImportsField::Map(entries) => {
                let mut map = serializer.serialize_map(Some(entries.len()))?;
                for (key, value) in entries {
                    map.serialize_entry(key, value)?;
                }
                map.end()
            }
        }
    }
}

pub struct ImportsFieldEntriesVisitor {
}

impl<'de> Visitor<'de> for ImportsFieldEntriesVisitor {
    type Value = ImportsField;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        write!(formatter, "a string or an object")
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E> where E: de::Error {
        let path = Path::try_from(value)
            .map_err(|err| de::Error::custom(err))?;

        Ok(ImportsField::Path(RawPath {path, raw: value.to_string()}))
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error> where A: de::MapAccess<'de> {
        let mut entries = vec![];

        while let Some(key) = map.next_key::<String>()? {
            entries.push((key, map.next_value::<ImportsField>()?));
        }

        Ok(ImportsField::Map(entries))
    }
}

pub struct ImportsFieldPathIter<'a> {
    stack: Vec<&'a ImportsField>,
}

impl<'a> ImportsFieldPathIter<'a> {
    fn new(root: &'a ImportsField) -> Self {
        ImportsFieldPathIter {
            stack: vec![root],
        }
    }
}

impl<'a> Iterator for ImportsFieldPathIter<'a> {
    type Item = &'a RawPath;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(current) = self.stack.pop() {
            match current {
                ImportsField::Path(path) => {
                    return Some(path);
                },

                ImportsField::Package(_) => {
                    // Import entries that map to a package don't have
                    // an associated path, so we just skip them
                },

                ImportsField::Map(entries) => {
                    for (_, child) in entries.iter().rev() {
                        self.stack.push(child);
                    }
                }
            }
        }

        None
    }
}
