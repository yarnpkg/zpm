use std::collections::BTreeMap;

use browser::BrowserField;
use zpm_switch::PackageManagerField;
use zpm_utils::Path;
use bin::BinField;
use bincode::{Decode, Encode};
use exports::ExportsField;
use imports::ImportsField;
use resolutions::ResolutionSelector;
use serde::{Deserialize, Serialize};

use crate::{primitives::{descriptor::{descriptor_map_deserializer, descriptor_map_serializer}, Descriptor, Ident, PeerRange, Range}, system};

pub mod bin;
pub mod browser;
pub mod exports;
pub mod helpers;
pub mod imports;
pub mod resolutions;

#[derive(Clone, Debug, Deserialize, Serialize, Encode, Decode)]
#[serde(rename_all = "camelCase")]
pub struct DistManifest {
    pub tarball: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, Encode, Decode)]
pub struct BinManifest {
    pub name: Option<Ident>,
    pub bin: Option<BinField>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, Encode, Decode)]
#[serde(rename_all = "camelCase")]
pub struct RemoteManifest {
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<zpm_semver::Version>,

    #[serde(flatten)]
    pub requirements: system::Requirements,

    #[serde(default)]
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    #[serde(serialize_with = "descriptor_map_serializer")]
    #[serde(deserialize_with = "descriptor_map_deserializer")]
    pub dependencies: BTreeMap<Ident, Descriptor>,

    #[serde(default)]
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub peer_dependencies: BTreeMap<Ident, PeerRange>,

    #[serde(default)]
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    #[serde(serialize_with = "descriptor_map_serializer")]
    #[serde(deserialize_with = "descriptor_map_deserializer")]
    pub optional_dependencies: BTreeMap<Ident, Descriptor>,

    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dist: Option<DistManifest>,
}


#[derive(Clone, Debug, Default, Deserialize, Serialize, Encode, Decode, PartialEq, Eq)]
#[serde(rename_all = "camelCase")] 
pub struct PublishConfig {
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "type")]
    pub type_: Option<String>,

    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub main: Option<String>,

    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exports: Option<ExportsField>,

    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub imports: Option<ImportsField>,

    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module: Option<String>,

    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub browser: Option<BrowserField>,

    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bin: Option<BinField>,

    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub executable_files: Option<Vec<Path>>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, Encode, Decode)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub package_manager: Option<PackageManagerField>,

    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<Ident>,

    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub type_: Option<String>,

    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub main: Option<String>,

    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exports: Option<ExportsField>,

    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub imports: Option<ImportsField>,

    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module: Option<String>,

    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub browser: Option<BrowserField>,

    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bin: Option<BinField>,

    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<String>>,

    #[serde(flatten)]
    pub remote: RemoteManifest,

    #[serde(default)]
    #[serde(skip_serializing_if = "zpm_utils::is_default")]
    pub publish_config: PublishConfig,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspaces: Option<Vec<String>>,

    #[serde(default)]
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    #[serde(serialize_with = "descriptor_map_serializer")]
    #[serde(deserialize_with = "descriptor_map_deserializer")]
    pub dev_dependencies: BTreeMap<Ident, Descriptor>,

    #[serde(default)]
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub scripts: BTreeMap<String, String>,

    #[serde(default)]
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub resolutions: BTreeMap<ResolutionSelector, Range>,
}

impl Manifest {
    pub fn iter_hard_dependencies(&self) -> impl Iterator<Item = (&Ident, &Descriptor)> {
        self.remote.dependencies.iter()
            .chain(self.remote.optional_dependencies.iter())
            .chain(self.dev_dependencies.iter())
    }

    pub fn iter_hard_dependencies_mut(&mut self) -> impl Iterator<Item = (&Ident, &mut Descriptor)> {
        self.remote.dependencies.iter_mut()
            .chain(self.remote.optional_dependencies.iter_mut())
            .chain(self.dev_dependencies.iter_mut())
    }
}
