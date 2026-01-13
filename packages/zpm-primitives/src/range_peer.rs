use std::convert::Infallible;

use bincode::{Decode, Encode};
use zpm_macro_enum::zpm_enum;
use zpm_utils::{DataType, Path, ToFileString};

use crate::{AnonymousSemverRange, Range, WorkspaceMagicRange, WorkspacePathRange, WorkspaceSemverRange};

type Error = Infallible;

#[zpm_enum(or_else = |_| Ok(PeerRange::Semver(SemverPeerRange {range: zpm_semver::Range::from_file_string("*").unwrap()})))]
#[derive(Clone, Debug, Decode, Encode, PartialEq, Eq, Hash)]
#[derive_variants(Clone, Debug, Decode, Encode, PartialEq, Eq, Hash)]
pub enum PeerRange {
    #[pattern(r"(?<range>.*)")]
    #[to_file_string(|| format!("{range}"))]
    #[to_print_string(|| DataType::Range.colorize(&format!("{}", range.to_file_string())))]
    Semver {
        range: zpm_semver::Range,
    },

    #[pattern(r"workspace:(?<magic>.*)")]
    #[to_file_string(|| format!("workspace:{magic}"))]
    #[to_print_string(|| DataType::Range.colorize(&format!("workspace:{}", magic.to_file_string())))]
    WorkspaceMagic {
        magic: zpm_semver::RangeKind,
    },

    #[pattern("workspace:(?<range>.*)")]
    #[to_file_string(|| format!("workspace:{range}"))]
    #[to_print_string(|| DataType::Range.colorize(&format!("workspace:{}", range.to_file_string())))]
    WorkspaceSemver {
        range: zpm_semver::Range,
    },

    #[pattern(r"workspace:(?<path>.*)")]
    #[to_file_string(|| format!("workspace:{path}"))]
    #[to_print_string(|| DataType::Range.colorize(&format!("workspace:{}", path.to_file_string())))]
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
