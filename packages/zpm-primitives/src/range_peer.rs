use std::convert::Infallible;

use bincode::{Decode, Encode};
use zpm_macro_enum::zpm_enum;
use zpm_utils::{impl_file_string_from_str, impl_file_string_serialization, DataType, Path, ToFileString, ToHumanString};

use crate::{AnonymousSemverRange, Range, WorkspaceMagicRange, WorkspacePathRange, WorkspaceSemverRange};

// #[zpm_enum(or_else = |s| Err(Error::InvalidIdentOrLocator(s.to_string())))]
// #[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
// #[derive_variants(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
// pub enum PackageSelector {
//     #[pattern(spec = "(?<ident>@?[^@]+)")]
//     Ident {
//         ident: Ident,
//     },

//     #[pattern(spec = "(?<ident>@?[^@]+)@(?<range>.*)")]
//     Range {
//         ident: Ident,
//         range: zpm_semver::Range,
//     },
// }

// impl PackageSelector {
//     pub fn ident(&self) -> &Ident {
//         match self {
//             PackageSelector::Ident(params) => &params.ident,
//             PackageSelector::Range(params) => &params.ident,
//         }
//     }
// }

// impl<'de> Deserialize<'de> for PackageSelector {
//     fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> where D: Deserializer<'de> {
//         let s = String::deserialize(deserializer)?;
//         PackageSelector::from_file_string(&s).map_err(serde::de::Error::custom)
//     }
// }

type Error = Infallible;

#[zpm_enum(or_else = |_| Ok(PeerRange::Semver(SemverPeerRange {range: zpm_semver::Range::from_file_string("*").unwrap()})))]
#[derive(Clone, Debug, Decode, Encode, PartialEq, Eq, Hash)]
#[derive_variants(Clone, Debug, Decode, Encode, PartialEq, Eq, Hash)]
pub enum PeerRange {
    #[pattern(spec = r"(?<range>.*)")]
    Semver {
        range: zpm_semver::Range,
    },

    #[pattern(spec = r"workspace:(?<magic>.*)")]
    WorkspaceMagic {
        magic: zpm_semver::RangeKind,
    },

    #[pattern(spec = "workspace:(?<range>.*)")]
    WorkspaceSemver {
        range: zpm_semver::Range,
    },

    #[pattern(spec = r"workspace:(?<path>.*)")]
    WorkspacePath {
        path: Path,
    }
}

impl PeerRange {
    pub fn to_range(&self) -> Range {
        match self {
            PeerRange::Semver(params) => {
                Range::AnonymousSemver(AnonymousSemverRange {range: params.range.clone()})
            },

            PeerRange::WorkspaceSemver(params) => {
                Range::WorkspaceSemver(WorkspaceSemverRange {range: params.range.clone()})
            },

            PeerRange::WorkspaceMagic(params) => {
                Range::WorkspaceMagic(WorkspaceMagicRange {magic: params.magic})
            },

            PeerRange::WorkspacePath(params) => {
                Range::WorkspacePath(WorkspacePathRange {path: params.path.clone()})
            },
        }
    }
}

impl ToFileString for PeerRange {
    fn to_file_string(&self) -> String {
        match self {
            PeerRange::Semver(params) => {
                params.range.to_file_string()
            },

            PeerRange::WorkspaceSemver(params) => {
                format!("workspace:{}", params.range.to_file_string())
            },

            PeerRange::WorkspaceMagic(params) => {
                format!("workspace:{}", params.magic.to_file_string())
            },

            PeerRange::WorkspacePath(params) => {
                format!("workspace:{}", params.path.to_file_string())
            },
        }
    }
}

impl ToHumanString for PeerRange {
    fn to_print_string(&self) -> String {
        DataType::Custom(0, 175, 175).colorize(&self.to_file_string())
    }
}

impl_file_string_from_str!(PeerRange);
impl_file_string_serialization!(PeerRange);
