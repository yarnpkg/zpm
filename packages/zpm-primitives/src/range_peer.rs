use std::convert::Infallible;

use rkyv::Archive;
use zpm_macro_enum::zpm_enum;
use zpm_utils::{DataType, Path, ToFileString};

use crate::{AnonymousSemverRange, CatalogRange, Range, WorkspaceMagicRange, WorkspacePathRange, WorkspaceSemverRange};

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
    },

    #[pattern(r"catalog:(?<catalog>.+)?")]
    #[to_file_string(|params| format!("catalog:{}", params.catalog.as_deref().unwrap_or("")))]
    #[to_print_string(|params| DataType::Range.colorize(&format!("catalog:{}", params.catalog.as_deref().unwrap_or(""))))]
    Catalog {
        catalog: Option<String>,
    },
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

            PeerRange::Catalog(params) => {
                Range::Catalog(CatalogRange {catalog: params.catalog.clone()})
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use zpm_utils::FromFileString;

    use super::*;

    #[rstest]
    #[case("catalog:", None)]
    #[case("catalog:react18", Some("react18"))]
    fn test_catalog_peer_range(#[case] str: &str, #[case] catalog: Option<&str>) {
        let range
            = PeerRange::from_file_string(str).unwrap();

        assert_eq!(range, PeerRange::Catalog(CatalogPeerRange {catalog: catalog.map(str::to_string)}));
        assert_eq!(range.to_file_string(), str);
        assert_eq!(range.to_range(), Range::from_file_string(str).unwrap());
    }

    #[rstest]
    #[case("^1.0.0")]
    #[case("workspace:^")]
    #[case("workspace:^1.0.0")]
    fn test_other_peer_ranges_arent_catalogs(#[case] str: &str) {
        let range
            = PeerRange::from_file_string(str).unwrap();

        assert!(!matches!(range, PeerRange::Catalog(_)), "{str} parsed as {range:?}");
        assert_eq!(range.to_file_string(), str);
    }
}
