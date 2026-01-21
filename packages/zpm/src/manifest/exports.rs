use std::fmt;

use rkyv::Archive;
use zpm_utils::{Path, RawPath};
use serde::{de::{self, Visitor}, ser::{SerializeMap, SerializeSeq}, Deserialize, Deserializer, Serialize, Serializer};

#[derive(Clone, Debug, PartialEq, Eq, Archive, rkyv::Serialize, rkyv::Deserialize)]
#[rkyv(serialize_bounds(__S: rkyv::ser::Writer + rkyv::ser::Allocator, <__S as rkyv::rancor::Fallible>::Error: rkyv::rancor::Source))]
#[rkyv(deserialize_bounds(<__D as rkyv::rancor::Fallible>::Error: rkyv::rancor::Source))]
#[rkyv(bytecheck(bounds(__C: rkyv::validation::ArchiveContext, <__C as rkyv::rancor::Fallible>::Error: rkyv::rancor::Source)))]
pub enum ExportsField {
    Null,
    Path(RawPath),
    Map(#[rkyv(omit_bounds)] Vec<(String, ExportsField)>),
    Array(#[rkyv(omit_bounds)] Vec<ExportsField>),
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
            ExportsField::Null => {
                serializer.serialize_none()
            },

            ExportsField::Path(raw_path) => {
                serializer.serialize_str(&raw_path.raw)
            },

            ExportsField::Map(entries) => {
                let mut map
                    = serializer.serialize_map(Some(entries.len()))?;

                for (key, value) in entries {
                    map.serialize_entry(key, value)?;
                }

                map.end()
            },

            ExportsField::Array(entries) => {
                let mut seq
                    = serializer.serialize_seq(Some(entries.len()))?;

                for value in entries {
                    seq.serialize_element(value)?;
                }

                seq.end()
            },
        }
    }
}

struct ExportsFieldVisitor {
}

impl<'de> Visitor<'de> for ExportsFieldVisitor {
    type Value = ExportsField;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        write!(formatter, "null, a string, or an object")
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> where E: de::Error {
        Ok(ExportsField::Null)
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E> where E: de::Error {
        let path = Path::try_from(value)
            .map_err(|err| de::Error::custom(err))?;

        Ok(ExportsField::Path(RawPath {path, raw: value.to_string()}))
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error> where A: de::SeqAccess<'de> {
        let mut entries = vec![];

        while let Some(value) = seq.next_element::<ExportsField>()? {
            entries.push(value);
        }

        Ok(ExportsField::Array(entries))
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
                ExportsField::Null => {
                    continue;
                },

                ExportsField::Path(path) => {
                    return Some(path);
                },

                ExportsField::Map(entries) => {
                    for (_, child) in entries.iter().rev() {
                        self.stack.push(child);
                    }
                },

                ExportsField::Array(entries) => {
                    for child in entries.iter().rev() {
                        self.stack.push(child);
                    }
                },
            }
        }

        None
    }
}
