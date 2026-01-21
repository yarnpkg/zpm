use std::collections::BTreeMap;

use rkyv::Archive;
use serde_with::serde_as;
use zpm_primitives::Ident;
use zpm_utils::{Path, RawPath};
use serde::{Deserialize, Serialize};

#[serde_as]
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Archive, rkyv::Serialize, rkyv::Deserialize)]
#[serde(untagged)]
pub enum BinField {
    String(RawPath),

    // Some registries incorrectly normalize the `bin` field of
    // scoped packages to be invalid filenames.
    //
    // E.g. from
    // {
    //   "name": "@yarnpkg/doctor",
    //   "bin": "index.js"
    // }
    // to
    // {
    //   "name": "@yarnpkg/doctor",
    //   "bin": {
    //     "@yarnpkg/doctor": "index.js"
    //   }
    // }
    //
    // To avoid that we always parse the `bin` keys as idents.
    Map(BTreeMap<Ident, RawPath>),
}

impl BinField {
    pub fn paths(&self) -> impl Iterator<Item = &Path> {
        match self {
            BinField::String(path) => vec![path].into_iter(),
            BinField::Map(map) => map.values().collect::<Vec<_>>().into_iter(),
        }.map(|p| &p.path)
    }
}

impl Iterator for BinField {
    type Item = (Ident, RawPath);

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            BinField::String(_) => None,
            BinField::Map(map) => map.iter().next().map(|(k, v)| (k.clone(), v.clone())),
        }
    }
}
