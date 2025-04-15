use std::fmt;

use zpm_utils::{Path, RawPath};
use bincode::{Decode, Encode};
use serde::{de::{self, Visitor}, ser::SerializeMap, Deserialize, Deserializer, Serialize, Serializer};

#[derive(Clone, Debug, Encode, Decode, PartialEq, Eq)]
pub enum ExportsField {
    Path(RawPath),
    Map(Vec<(String, ExportsField)>),
}

impl<'a> ExportsField {
    pub fn paths(&'a self) -> impl Iterator<Item = &'a RawPath> {
        ExportsFieldPathIter::new(self)
    }
}

impl<'de> Deserialize<'de> for ExportsField {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> where D: Deserializer<'de> {
        deserializer.deserialize_any(ExportsFieldVisitor {})
    }
}

impl Serialize for ExportsField {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where S: Serializer {
        match self {
            ExportsField::Path(raw_path) => serializer.serialize_str(&raw_path.raw),
            ExportsField::Map(entries) => {
                let mut map = serializer.serialize_map(Some(entries.len()))?;
                for (key, value) in entries {
                    map.serialize_entry(key, value)?;
                }
                map.end()
            }
        }
    }
}

struct ExportsFieldVisitor {
}

impl<'de> Visitor<'de> for ExportsFieldVisitor {
    type Value = ExportsField;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        write!(formatter, "a string or an object")
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E> where E: de::Error {
        let path = Path::try_from(value)
            .map_err(|err| de::Error::custom(err))?;

        Ok(ExportsField::Path(RawPath {path, raw: value.to_string()}))
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error> where A: de::MapAccess<'de> {
        let mut entries = vec![];

        while let Some(key) = map.next_key::<String>()? {
            entries.push((key, map.next_value::<ExportsField>()?));
        }

        Ok(ExportsField::Map(entries))
    }
}

pub struct ExportsFieldPathIter<'a> {
    stack: Vec<&'a ExportsField>,
}

impl<'a> ExportsFieldPathIter<'a> {
    fn new(root: &'a ExportsField) -> Self {
        ExportsFieldPathIter {
            stack: vec![root],
        }
    }
}

impl<'a> Iterator for ExportsFieldPathIter<'a> {
    type Item = &'a RawPath;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(current) = self.stack.pop() {
            match current {
                ExportsField::Path(path) => return Some(path),
                ExportsField::Map(entries) => {
                    for (_, child) in entries.iter().rev() {
                        self.stack.push(child);
                    }
                }
            }
        }

        None
    }
}
