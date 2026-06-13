use std::collections::BTreeMap;

use browser::BrowserField;
use rkyv::Archive;
use serde_with::{serde_as, DefaultOnError};
use zpm_parsers::{Document, Value};
use zpm_primitives::{Descriptor, Ident, PeerRange, descriptor_map_deserializer, descriptor_map_serializer};
use zpm_switch::PackageManagerField;
use zpm_utils::{Path, Requirements, ToFileString};
use bin::BinField;
use exports::ExportsField;
use imports::ImportsField;
use resolutions::ResolutionsField;
use serde::{Deserialize, Serialize};

pub mod bin;
pub mod browser;
pub mod exports;
pub mod helpers;
pub mod imports;
pub mod resolutions;

#[derive(Clone, Debug, Deserialize, Serialize, Archive, rkyv::Serialize, rkyv::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DistManifest {
    pub tarball: String,
}

/// Accepts the array form and the deprecated object form
/// (`{ packages, nohoist }`). `nohoist` is retained so install can
/// warn about each pattern even though zpm doesn't honor it.
#[derive(Clone, Debug, Default, Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct WorkspacesField {
    pub packages: Vec<String>,
    pub nohoist: Vec<String>,
}

impl WorkspacesField {
    pub fn is_empty(&self) -> bool {
        self.packages.is_empty() && self.nohoist.is_empty()
    }
}

impl<'de> Deserialize<'de> for WorkspacesField {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Either {
            Array(Vec<String>),
            Object {
                #[serde(default)]
                packages: Vec<String>,
                #[serde(default)]
                nohoist: Vec<String>,
            },
        }

        Ok(match Either::deserialize(deserializer)? {
            Either::Array(packages) => WorkspacesField { packages, nohoist: Vec::new() },
            Either::Object { packages, nohoist } => WorkspacesField { packages, nohoist },
        })
    }
}

impl Serialize for WorkspacesField {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // Only emit the packages array — round-tripping `nohoist`
        // would pin the deprecated shape into manifests.
        self.packages.serialize(serializer)
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, Archive, rkyv::Serialize, rkyv::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hoisting_limits: Option<HoistingLimitsValue>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub self_references: Option<bool>,
}

/// Rkyv-aware mirror of `zpm_config::NmHoistingLimits` (zpm-config
/// doesn't pull in rkyv).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, Archive, rkyv::Serialize, rkyv::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HoistingLimitsValue {
    None,
    Workspaces,
    Dependencies,
}

impl From<HoistingLimitsValue> for zpm_config::NmHoistingLimits {
    fn from(value: HoistingLimitsValue) -> Self {
        match value {
            HoistingLimitsValue::None => zpm_config::NmHoistingLimits::None,
            HoistingLimitsValue::Workspaces => zpm_config::NmHoistingLimits::Workspaces,
            HoistingLimitsValue::Dependencies => zpm_config::NmHoistingLimits::Dependencies,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct BinManifest {
    pub name: Option<Ident>,
    pub bin: Option<BinField>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct PeerDependenciesMeta {
    pub optional: bool,
}

#[serde_as]
#[derive(Clone, Debug, Default, Deserialize, Serialize, Archive, rkyv::Serialize, rkyv::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteManifest {
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde_as(deserialize_as = "DefaultOnError")]
    pub version: Option<zpm_semver::Version>,

    #[serde(flatten)]
    pub requirements: Requirements,

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
    pub peer_dependencies_meta: BTreeMap<Ident, PeerDependenciesMeta>,

    #[serde(default)]
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    #[serde(serialize_with = "descriptor_map_serializer")]
    #[serde(deserialize_with = "descriptor_map_deserializer")]
    pub optional_dependencies: BTreeMap<Ident, Descriptor>,

    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dist: Option<DistManifest>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq, Archive, rkyv::Serialize, rkyv::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishConfig {
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access: Option<ManifestNpmPublishAccess>,

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

    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub types: Option<String>,

    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub typings: Option<String>,

    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provenance: Option<bool>,

    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub registry: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, Archive, rkyv::Serialize, rkyv::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ManifestNpmPublishAccess {
    Public,
    Restricted,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, Archive, rkyv::Serialize, rkyv::Deserialize)]
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
    pub stable_version: Option<zpm_semver::Version>,

    #[serde(default)]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub extends: Vec<String>,

    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub private: Option<bool>,

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

    #[serde(default, skip_serializing_if = "WorkspacesField::is_empty")]
    pub workspaces: WorkspacesField,

    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub install_config: Option<InstallConfig>,

    #[serde(default)]
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    #[serde(serialize_with = "descriptor_map_serializer")]
    #[serde(deserialize_with = "descriptor_map_deserializer")]
    pub dev_dependencies: BTreeMap<Ident, Descriptor>,

    #[serde(default)]
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub scripts: BTreeMap<String, String>,

    #[serde(default)]
    #[serde(skip_serializing_if = "ResolutionsField::is_empty")]
    pub resolutions: ResolutionsField,
}

#[derive(Debug, Clone, Copy)]
pub enum HardDependencyKind {
    Dependency,
    OptionalDependency,
    DevDependency,
}

impl HardDependencyKind {
    pub fn to_str(&self) -> &str {
        match self {
            HardDependencyKind::Dependency => "dependencies",
            HardDependencyKind::OptionalDependency => "optionalDependencies",
            HardDependencyKind::DevDependency => "devDependencies",
        }
    }
}

impl HardDependencyKind {
    pub fn insert_into<D: Document>(self, document: &mut D, descriptor: &Descriptor) -> Result<(), zpm_parsers::Error> {
        document.set_path(
            &zpm_parsers::Path::from_segments(vec![
                self.to_str().to_string(),
                descriptor.ident.to_file_string(),
            ]),
            Value::String(descriptor.range.to_file_string()),
        )
    }
}

#[derive(Debug, Clone)]
pub struct HardDependency<'a> {
    pub kind: HardDependencyKind,
    pub ident: &'a Ident,
    pub descriptor: &'a Descriptor,
}

#[derive(Debug, Clone)]
pub struct PeerDependency<'a> {
    pub ident: &'a Ident,
    pub range: &'a PeerRange,
}

impl Manifest {
    pub fn iter_hard_dependencies(&self) -> impl Iterator<Item = HardDependency<'_>> {
        let dependencies_iter = self.remote.dependencies.iter()
            .map(|(ident, descriptor)| HardDependency {
                kind: HardDependencyKind::Dependency,
                ident,
                descriptor,
            });

        let optional_dependencies_iter = self.remote.optional_dependencies.iter()
            .map(|(ident, descriptor)| HardDependency {
                kind: HardDependencyKind::OptionalDependency,
                ident,
                descriptor,
            });

        let dev_dependencies_iter = self.dev_dependencies.iter()
            .map(|(ident, descriptor)| HardDependency {
                kind: HardDependencyKind::DevDependency,
                ident,
                descriptor,
            });

        dependencies_iter.chain(optional_dependencies_iter).chain(dev_dependencies_iter)
    }

    pub fn iter_peer_dependencies(&self) -> impl Iterator<Item = PeerDependency<'_>> {
        self.remote.peer_dependencies.iter()
            .map(|(ident, range)| PeerDependency { ident, range })
    }
}
