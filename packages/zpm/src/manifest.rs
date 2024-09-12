use std::{collections::HashMap, fs, sync::Arc};

use arca::Path;
use serde::{Deserialize, Serialize};
use serde_with::serde_as;

use crate::{error::Error, primitives::{descriptor::{descriptor_map_deserializer, descriptor_map_serializer}, Descriptor, Ident, PeerRange}, semver, system};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DistManifest {
    pub tarball: String,
}

#[serde_as]
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum BinField {
    String(Path),
    Map(HashMap<String, Path>),
}

impl Iterator for BinField {
    type Item = (String, Path);

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            BinField::String(_) => None,
            BinField::Map(map) => map.iter().next().map(|(k, v)| (k.clone(), v.clone())),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BinManifest {
    pub name: Option<Ident>,
    pub bin: Option<BinField>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteManifest {
    #[serde(default)]
    pub version: semver::Version,

    #[serde(flatten)]
    pub requirements: system::Requirements,

    #[serde(default)]
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    #[serde(serialize_with = "descriptor_map_serializer")]
    #[serde(deserialize_with = "descriptor_map_deserializer")]
    pub dependencies: HashMap<Ident, Descriptor>,

    #[serde(default)]
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub peer_dependencies: HashMap<Ident, PeerRange>,

    #[serde(default)]
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    #[serde(serialize_with = "descriptor_map_serializer")]
    #[serde(deserialize_with = "descriptor_map_deserializer")]
    pub optional_dependencies: HashMap<Ident, Descriptor>,

    #[serde(default)]
    pub dist: Option<DistManifest>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<Ident>,

    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub main: Option<String>,

    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module: Option<String>,

    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub browser: Option<String>,

    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bin: Option<BinField>,

    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<String>>,

    #[serde(flatten)]
    pub remote: RemoteManifest,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspaces: Option<Vec<String>>,

    #[serde(default)]
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    #[serde(serialize_with = "descriptor_map_serializer")]
    #[serde(deserialize_with = "descriptor_map_deserializer")]
    pub dev_dependencies: HashMap<Ident, Descriptor>,

    #[serde(default)]
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub scripts: HashMap<String, String>,
}

pub fn parse_manifest(manifest_text: String) -> Result<Manifest, Error> {
    let manifest_data = serde_json::from_str(manifest_text.as_str())
        .map_err(Arc::new)?;

    Ok(manifest_data)
}

pub fn read_manifest(p: &Path) -> Result<Manifest, Error> {
    let manifest_text = fs::read_to_string(p.to_path_buf())?;
    let manifest_data = serde_json::from_str(manifest_text.as_str())?;

    Ok(manifest_data)
}
