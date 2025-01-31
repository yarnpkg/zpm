use std::{collections::BTreeMap, io::{self, ErrorKind}};

use arca::Path;
use bincode::{Decode, Encode};
use serde::{Deserialize, Serialize};
use serde_with::serde_as;
use zpm_macros::parse_enum;

use crate::{error::Error, primitives::{descriptor::{descriptor_map_deserializer, descriptor_map_serializer}, Descriptor, Ident, Locator, PeerRange, Range}, semver::{self, Version}, system};

#[derive(Clone, Debug, Deserialize, Serialize, Encode, Decode)]
#[serde(rename_all = "camelCase")]
pub struct DistManifest {
    pub tarball: String,
}

#[serde_as]
#[derive(Debug, Clone, Deserialize, Serialize, Encode, Decode)]
#[serde(untagged)]
pub enum BinField {
    String(Path),
    Map(BTreeMap<String, Path>),
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

#[derive(Debug, Clone, Deserialize, Serialize, Encode, Decode)]
pub struct BinManifest {
    pub name: Option<Ident>,
    pub bin: Option<BinField>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, Encode, Decode)]
#[serde(rename_all = "camelCase")]
pub struct RemoteManifest {
    #[serde(default)]
    pub version: semver::Version,

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
    pub dist: Option<DistManifest>,
}

#[parse_enum(or_else = |s| Err(Error::InvalidResolution(s.to_string())))]
#[derive(Clone, Debug, Serialize, PartialEq, Eq, Hash, PartialOrd, Ord, Encode, Decode)]
#[derive_variants(Clone, Debug, Serialize, PartialEq, Eq, Hash, PartialOrd, Ord, Encode, Decode)]
pub enum ResolutionOverride {
    #[pattern(spec = r"^(?<descriptor>.*)$")]
    Descriptor {
        descriptor: Descriptor,
    },

    #[pattern(spec = r"^(?<ident>.*)$")]
    Ident {
        ident: Ident
    },

    #[pattern(spec = r"^(?<parent_descriptor>(?:@[^/*]*/)?[^/*]+)/(?<ident>[^*]+)$")]
    DescriptorIdent {
        parent_descriptor: Descriptor,
        ident: Ident,
    },

    #[pattern(spec = r"^(?<parent_ident>(?:@[^/*]*/)?[^/*]+)/(?<ident>[^*]+)$")]
    IdentIdent {
        parent_ident: Ident,
        ident: Ident,
    },
}

impl ResolutionOverride {
    pub fn target_ident(&self) -> &Ident {
        match self {
            ResolutionOverride::Descriptor(params) => &params.descriptor.ident,
            ResolutionOverride::Ident(params) => &params.ident,
            ResolutionOverride::DescriptorIdent(params) => &params.ident,
            ResolutionOverride::IdentIdent(params) => &params.ident,
        }
    }

    pub fn apply(&self, parent: &Locator, parent_version: &Version, descriptor: &Descriptor, replacement_range: &Range) -> Option<Range> {
        match self {
            ResolutionOverride::Descriptor(params) => {
                if params.descriptor != *descriptor {
                    return None;
                }

                Some(replacement_range.clone())
            }

            ResolutionOverride::Ident(params) => {
                if params.ident != descriptor.ident {
                    return None;
                }

                Some(replacement_range.clone())
            }

            ResolutionOverride::DescriptorIdent(params) => {
                if params.ident != descriptor.ident {
                    return None;
                }

                if let Range::AnonymousSemver(parent_params) = &params.parent_descriptor.range {
                    if !parent_params.range.check(parent_version) {
                        return None;
                    }
                } else {
                    return None;
                }

                Some(replacement_range.clone())
            }

            ResolutionOverride::IdentIdent(params) => {
                if params.ident != descriptor.ident {
                    return None;
                }

                if params.parent_ident != parent.ident {
                    return None;
                }

                Some(replacement_range.clone())
            }
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, Encode, Decode)]
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
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    #[serde(serialize_with = "descriptor_map_serializer")]
    #[serde(deserialize_with = "descriptor_map_deserializer")]
    pub dev_dependencies: BTreeMap<Ident, Descriptor>,

    #[serde(default)]
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub scripts: BTreeMap<String, String>,

    #[serde(default)]
    pub resolutions: BTreeMap<ResolutionOverride, Range>,
}

fn wrap_error<T>(result: Result<T, io::Error>) -> Result<T, Error> {
    result.map_err(|err| match err.kind() {
        ErrorKind::NotFound | ErrorKind::NotADirectory => Error::ManifestNotFound,
        _ => err.into(),
    })
}

pub fn read_manifest(abs_path: &Path) -> Result<Manifest, Error> {
    let metadata = wrap_error(abs_path.fs_metadata())?;

    Ok(read_manifest_with_size(abs_path, metadata.len())?)
}

pub fn read_manifest_with_size(abs_path: &Path, size: u64) -> Result<Manifest, Error> {
    let manifest_text = wrap_error(abs_path.fs_read_text_with_size(size))?;

    parse_manifest(&manifest_text)
}

pub fn parse_manifest(manifest_text: &str) -> Result<Manifest, Error> {
    if manifest_text.len() > 0 {
        Ok(sonic_rs::from_str(&manifest_text)?)
    } else {
        Ok(Manifest::default())
    }
}
