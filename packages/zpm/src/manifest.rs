use std::{collections::HashMap, fs, str::FromStr, sync::Arc};

use arca::Path;
use serde::{Deserialize, Deserializer};

use crate::{error::Error, primitives::{Descriptor, Ident, PeerRange, Range}, semver};

fn from_dependency_map<'de, D>(deserializer: D) -> Result<Option<HashMap<Ident, Descriptor>>, D::Error> where D: Deserializer<'de> {
    let source: Option<HashMap<String, String>> = Deserialize::deserialize(deserializer)?;

    if let Some(source) = source {
        let mut entries = HashMap::new();

        for (k, v) in source.iter() {
            let range = Range::from_str(v)
                .map_err(serde::de::Error::custom)?;

            let ident = Ident::new(k);
            let descriptor = Descriptor::new(Ident::new(k), range);

            entries.insert(ident, descriptor);
        }

        Ok(Some(entries))
    } else {
        Ok(None)
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteManifest {
    pub version: semver::Version,

    #[serde(default)]
    #[serde(deserialize_with = "from_dependency_map")]
    pub dependencies: Option<HashMap<Ident, Descriptor>>,

    #[serde(default)]
    pub peer_dependencies: Option<HashMap<Ident, PeerRange>>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    #[serde(default)]
    pub name: Option<Ident>,

    #[serde(default)]
    pub version: semver::Version,

    pub workspaces: Option<Vec<String>>,

    #[serde(default)]
    #[serde(deserialize_with = "from_dependency_map")]
    pub dependencies: Option<HashMap<Ident, Descriptor>>,

    #[serde(default)]
    #[serde(deserialize_with = "from_dependency_map")]
    pub dev_dependencies: Option<HashMap<Ident, Descriptor>>,

    #[serde(default)]
    pub peer_dependencies: Option<HashMap<Ident, PeerRange>>,
}

pub fn parse_manifest(manifest_text: String) -> Result<Manifest, Error> {
    let manifest_data = serde_json::from_str(manifest_text.as_str())
        .map_err(Arc::new)?;

    Ok(manifest_data)
}

pub fn read_manifest(p: &Path) -> Result<Manifest, Error> {
    let manifest_text = fs::read_to_string(p.to_path_buf())
        .map_err(Arc::new)?;

    let manifest_data = serde_json::from_str(manifest_text.as_str())
        .map_err(Arc::new)?;

    Ok(manifest_data)
}
