use std::collections::BTreeMap;

use rkyv::Archive;
use serde_with::serde_as;
use zpm_utils::{Path, RawPath};
use bincode::{Decode, Encode};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize, Encode, Decode, PartialEq, Eq, Archive, rkyv::Serialize, rkyv::Deserialize)]
#[serde(untagged)]
pub enum BrowserFieldEntry {
    Ignore(bool),
    Path(RawPath),
}

#[serde_as]
#[derive(Debug, Clone, Deserialize, Serialize, Encode, Decode, PartialEq, Eq, Archive, rkyv::Serialize, rkyv::Deserialize)]
#[serde(untagged)]
pub enum BrowserField {
    String(RawPath),
    Map(BTreeMap<String, BrowserFieldEntry>),
}

impl BrowserField {
    pub fn paths(&self) -> impl Iterator<Item = &Path> {
        match self {
            BrowserField::String(path)
                => vec![path].into_iter(),

            BrowserField::Map(map)
                => map.values()
                    .filter_map(|entry| match entry {
                        BrowserFieldEntry::Path(path) => Some(path),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .into_iter(),
        }.map(|p| &p.path)
    }
}

impl Iterator for BrowserField {
    type Item = (String, bool);

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            BrowserField::String(_)
                => None,

            BrowserField::Map(map)
                => map.iter()
                    .next()
                    .map(|(k, v)| (k.clone(), match v {
                        BrowserFieldEntry::Ignore(ignore) => *ignore,
                        BrowserFieldEntry::Path(_) => false,
                    })),
        }
    }
}
