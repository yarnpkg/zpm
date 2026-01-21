use std::convert::Infallible;

use rkyv::Archive;
use zpm_macro_enum::zpm_enum;
use zpm_utils::{DataType, Path, ToFileString};

use crate::{AnonymousSemverRange, Range, WorkspaceMagicRange, WorkspacePathRange, WorkspaceSemverRange};

type Error = Infallible;

#[zpm_enum(or_else = |_| Ok(PeerRange::Semver(SemverPeerRange {range: zpm_semver::Range::from_file_string("*").unwrap()})))]
#[derive(Clone, Debug, PartialEq, Eq, Hash, Archive, rkyv::Serialize, rkyv::Deserialize)]
#[derive_variants(Clone, Debug, PartialEq, Eq, Hash, Archive, rkyv::Serialize, rkyv::Deserialize)]
#[variant_struct_attr(rkyv(derive(PartialEq, Eq, Hash)))]
pub enum PeerRange {
    #[pattern(r"(?<range>.*)")]
    #[to_file_string(|params| params.range.to_file_string())]
    #[to_print_string(|params| DataType::Range.colorize(&params.range.to_file_string()))]
    Semver {
        range: zpm_semver::Range,
    },

    #[pattern(r"workspace:(?<magic>.*)")]
    #[to_file_string(|params| format!("workspace:{}", params.magic.to_file_string()))]
    #[to_print_string(|params| DataType::Range.colorize(&format!("workspace:{}", params.magic.to_file_string())))]
    WorkspaceMagic {
        magic: zpm_semver::RangeKind,
    },

    #[pattern("workspace:(?<range>.*)")]
    #[to_file_string(|params| format!("workspace:{}", params.range.to_file_string()))]
    #[to_print_string(|params| DataType::Range.colorize(&format!("workspace:{}", params.range.to_file_string())))]
    WorkspaceSemver {
        range: zpm_semver::Range,
    },

    #[pattern(r"workspace:(?<path>.*)")]
    #[to_file_string(|params| format!("workspace:{}", params.path.to_file_string()))]
    #[to_print_string(|params| DataType::Range.colorize(&format!("workspace:{}", params.path.to_file_string())))]
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
