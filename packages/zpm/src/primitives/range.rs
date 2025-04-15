use std::{hash::Hash, sync::LazyLock};

use zpm_utils::Path;
use bincode::{Decode, Encode};
use colored::Colorize;
use regex::Regex;
use serde::{Deserialize, Deserializer};
use zpm_macros::parse_enum;
use zpm_utils::{impl_serialization_traits, FromFileString, ToFileString, ToHumanString};

use crate::{error::Error, git, hash::Sha256, serialize::UrlEncoded};

use super::{Descriptor, Ident};

pub static EXPLICIT_PATH_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^.{0,2}/").unwrap()
});

#[parse_enum(or_else = |s| Err(Error::InvalidRange(s.to_string())))]
#[derive(Clone, Debug, Decode, Encode, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[derive_variants(Clone, Debug, Decode, Encode, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Range {
    #[pattern(spec = r"missing!")]
    MissingPeerDependency {},

    #[pattern(spec = r"(?<range>.*)")]
    AnonymousSemver {
        range: zpm_semver::Range,
    },

    #[pattern(spec = r"npm:(?:(?<ident>.*)@)?(?<range>.*)")]
    RegistrySemver {
        ident: Option<Ident>,
        range: zpm_semver::Range,
    },

    #[pattern(spec = r"npm:(?:(?<ident>.*)@)?(?<tag>[-a-z0-9._^v][-a-z0-9._]*)")]
    RegistryTag {
        ident: Option<Ident>,
        tag: String,
    },

    #[pattern(spec = r"link:(?<path>.*)")]
    Link {
        path: String,
    },

    #[pattern(spec = r"portal:(?<path>.*)")]
    Portal {
        path: String,
    },

    #[pattern(spec = r"file:(?<path>.*\.(?:tgz|tar\.gz))")]
    #[pattern(spec = r"(?<path>\.{0,2}/.*\.(?:tgz|tar\.gz))")]
    Tarball {
        path: String,
    },

    #[pattern(spec = r"file:(?<path>.*)")]
    #[pattern(spec = r"(?<path>\.{0,2}/.*)")]
    Folder {
        path: String,
    },

    #[pattern(spec = r"patch:(?<inner>.*)#(?<path>.*)$")]
    Patch {
        inner: Box<UrlEncoded<Descriptor>>,
        path: String,
    },

    #[pattern(spec = r"virtual:(?<inner>.*)#(?<hash>[a-f0-9]*)$")]
    Virtual {
        inner: Box<Range>,
        hash: Sha256,
    },

    #[pattern(spec = r"workspace:(?<magic>.*)")]
    WorkspaceMagic {
        magic: zpm_semver::RangeKind,
    },

    #[pattern(spec = r"workspace:(?<range>.*)")]
    WorkspaceSemver {
        range: zpm_semver::Range,
    },

    #[pattern(spec = r"workspace:\((?<ident>.*)\)")]
    WorkspaceIdent {
        ident: Ident,
    },

    #[pattern(spec = r"workspace:(?<path>.*)")]
    WorkspacePath {
        path: Path,
    },

    #[pattern(spec = "(?<git>.*)")]
    Git {
        git: git::GitRange,
    },

    #[pattern(spec = r"(?<url>https?://.*(?:/.*|\.tgz|\.tar\.gz))")]
    Url {
        url: String,
    },

    #[pattern(spec = r"(?<tag>.*)")]
    AnonymousTag {
        tag: String,
    },
}

impl Range {
    pub fn must_bind(&self) -> bool {
        // Keep this list in sync w/ Reference::must_bind
        matches!(&self, Range::Link(_) | Range::Portal(_) | Range::Tarball(_) | Range::Folder(_) | Range::Patch(_))
    }

    pub fn must_fetch_before_resolve(&self) -> bool {
        matches!(&self, Range::Git(_) | Range::Folder(_) | Range::Tarball(_) | Range::Url(_))
    }

    pub fn is_transient_resolution(&self) -> bool {
        matches!(&self, Range::Link(_) | Range::Portal(_) | Range::Tarball(_) | Range::Folder(_) | Range::Patch(_) | Range::WorkspaceIdent(_) | Range::WorkspaceMagic(_) | Range::WorkspacePath(_) | Range::WorkspaceSemver(_))
    }

    pub fn to_semver_range(&self) -> Option<zpm_semver::Range> {
        match self {
            Range::AnonymousSemver(params) => Some(params.range.clone()),
            Range::RegistrySemver(params) => Some(params.range.clone()),
            _ => None,
        }
    }

    pub fn to_peer_range(&self) -> Result<PeerRange, Error> {
        match self {
            Range::AnonymousSemver(params)
                => Ok(PeerRange::Semver(SemverPeerRange { range: params.range.clone() })),

            Range::RegistrySemver(params)
                => Ok(PeerRange::Semver(SemverPeerRange { range: params.range.clone() })),

            _ => Err(Error::InvalidRange(self.to_file_string()))
        }
    }
}

impl ToFileString for Range {
    fn to_file_string(&self) -> String {
        match self {
            Range::AnonymousSemver(params) => params.range.to_file_string(),
            Range::AnonymousTag(params) => params.tag.clone(),

            Range::RegistrySemver(params) => match &params.ident {
                Some(ident) => format!("npm:{}@{}", ident.to_file_string(), params.range.to_file_string()),
                None => format!("npm:{}", params.range.to_file_string()),
            },

            Range::RegistryTag(params) => match &params.ident {
                Some(ident) => format!("npm:{}@{}", ident.to_file_string(), params.tag),
                None => format!("npm:{}", params.tag),
            },

            Range::Tarball(params) => {
                if EXPLICIT_PATH_REGEX.is_match(params.path.as_str()) {
                    params.path.clone()
                } else {
                    format!("file:{}", params.path)
                }
            },

            Range::Folder(params) => {
                if EXPLICIT_PATH_REGEX.is_match(params.path.as_str()) {
                    params.path.clone()
                } else {
                    format!("file:{}", params.path)
                }
            },

            Range::Patch(params) => format!("patch:{}#{}", params.inner.to_file_string(), params.path),
            Range::Link(params) => format!("link:{}", params.path),
            Range::Portal(params) => format!("portal:{}", params.path),

            Range::Url(params) => params.url.clone(),
            Range::WorkspaceSemver(params) => format!("workspace:{}", params.range.to_file_string()),
            Range::WorkspaceMagic(params) => format!("workspace:{}", serde_plain::to_string(&params.magic).unwrap()),
            Range::WorkspacePath(params) => format!("workspace:{}", params.path),
            Range::WorkspaceIdent(params) => format!("workspace:{}", params.ident.to_file_string()),
            Range::Git(params) => params.git.to_file_string(),
            Range::MissingPeerDependency(_) => "missing!".to_string(),
            Range::Virtual(params) => format!("virtual:{}#{}", params.inner.to_file_string(), params.hash.to_file_string()),
        }
    }
}

impl ToHumanString for Range {
    fn to_print_string(&self) -> String {
        self.to_file_string().truecolor(0, 175, 175).to_string()
    }
}

impl_serialization_traits!(Range);

#[parse_enum(or_else = |s| Err(Error::InvalidIdentOrLocator(s.to_string())))]
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[derive_variants(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PackageSelector {
    #[pattern(spec = "(?<ident>@?[^@]+)")]
    Ident {
        ident: Ident,
    },

    #[pattern(spec = "(?<ident>@?[^@]+)@(?<range>.*)")]
    Range {
        ident: Ident,
        range: zpm_semver::Range,
    },
}

impl PackageSelector {
    pub fn ident(&self) -> &Ident {
        match self {
            PackageSelector::Ident(params) => &params.ident,
            PackageSelector::Range(params) => &params.ident,
        }
    }
}

impl<'de> Deserialize<'de> for PackageSelector {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> where D: Deserializer<'de> {
        let s = String::deserialize(deserializer)?;
        PackageSelector::from_file_string(&s).map_err(serde::de::Error::custom)
    }
}

#[parse_enum(or_else = |_| Ok(PeerRange::Semver(SemverPeerRange {range: zpm_semver::Range::from_file_string("*").unwrap()})))]
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

impl ToFileString for PeerRange {
    fn to_file_string(&self) -> String {
        match self {
            PeerRange::Semver(params) => params.range.to_file_string(),
            PeerRange::WorkspaceSemver(params) => format!("workspace:{}", params.range.to_file_string()),
            PeerRange::WorkspaceMagic(params) => format!("workspace:{}", params.magic),
            PeerRange::WorkspacePath(params) => format!("workspace:{}", params.path),
        }
    }
}

impl ToHumanString for PeerRange {
    fn to_print_string(&self) -> String {
        self.to_file_string().truecolor(0, 175, 175).to_string()
    }
}

impl_serialization_traits!(PeerRange);
