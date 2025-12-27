use std::{hash::Hash, str::FromStr, sync::LazyLock};

use bincode::{Decode, Encode};
use regex::Regex;
use zpm_macro_enum::zpm_enum;
use zpm_utils::{impl_file_string_from_str, impl_file_string_serialization, DataType, Hash64, Path, ToFileString, ToHumanString, UrlEncoded};

use crate::{PeerRange, SemverPeerRange};

use super::{Descriptor, Ident};

pub static EXPLICIT_PATH_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^.{0,2}/").unwrap()
});

#[derive(thiserror::Error, Clone, Debug)]
pub enum RangeError {
    #[error("Invalid range: {0}")]
    SyntaxError(String),

    #[error("Parsing error: {0}")]
    SemverError(#[from] zpm_semver::Error),

    #[error("Cannot convert range to peer range: {0}")]
    PeerRangeError(String),

    #[error(transparent)]
    FromUtf8Error(#[from] std::string::FromUtf8Error),
}

#[zpm_enum(error = RangeError, or_else = |s| Err(RangeError::SyntaxError(s.to_string())))]
#[derive(Clone, Debug, Decode, Encode, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[derive_variants(Clone, Debug, Decode, Encode, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Range {
    #[no_pattern]
    MissingPeerDependency,

    #[pattern(spec = r"builtin:(?<range>.*)")]
    Builtin {
        range: zpm_semver::Range,
    },

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

    #[pattern(spec = r"workspace:(?<magic>.*)")]
    WorkspaceMagic {
        magic: zpm_semver::RangeKind,
    },

    #[pattern(spec = r"workspace:(?<range>.*)")]
    WorkspaceSemver {
        range: zpm_semver::Range,
    },

    #[pattern(spec = r"workspace:(?<ident>.*)")]
    WorkspaceIdent {
        ident: Ident,
    },

    #[pattern(spec = r"workspace:(?<path>.*)")]
    WorkspacePath {
        path: Path,
    },

    #[pattern(spec = "(?<git>.*)")]
    Git {
        git: zpm_git::GitRange,
    },

    #[pattern(spec = r"(?<url>https?://.*(?:/.*|\.tgz|\.tar\.gz))")]
    Url {
        url: String,
    },

    #[pattern(spec = r"(?<tag>.*)")]
    AnonymousTag {
        tag: String,
    },

    // We keep this at the end so virtual ranges are listed last when sorted
    #[pattern(spec = r"virtual:(?<inner>.*)#(?<hash>[a-f0-9]*)$")]
    Virtual {
        inner: Box<Range>,
        hash: Hash64,
    },
}

impl Range {
    pub fn new_semver(range_str: &str) -> Result<Range, RangeError> {
        let range
            = zpm_semver::Range::from_str(range_str)?;

        Ok(Range::AnonymousSemver(AnonymousSemverRange {range}.into()))
    }

    pub fn inner_descriptor(&self) -> Option<Descriptor> {
        match self {
            Range::RegistrySemver(params) if params.ident.is_some()
                => Some(Descriptor::new(params.ident.clone().unwrap(), RegistrySemverRange {ident: None, range: params.range.clone()}.into())),

            Range::RegistryTag(params) if params.ident.is_some()
                => Some(Descriptor::new(params.ident.clone().unwrap(), RegistryTagRange {ident: None, tag: params.tag.clone()}.into())),

            Range::Patch(params)
                => Some(params.inner.0.clone()),

            _ => None,
        }
    }

    pub fn physical_range(&self) -> &Range {
        if let Range::Virtual(params) = self {
            params.inner.physical_range()
        } else {
            self
        }
    }

    pub fn is_workspace(&self) -> bool {
        matches!(self, Range::WorkspaceMagic(_) | Range::WorkspaceSemver(_) | Range::WorkspaceIdent(_) | Range::WorkspacePath(_))
    }

    pub fn to_anonymous_range(&self) -> Range {
        match self {
            Range::RegistrySemver(params) => {
                Range::AnonymousSemver(AnonymousSemverRange {range: params.range.clone()})
            },

            Range::RegistryTag(params) => {
                Range::AnonymousTag(AnonymousTagRange {tag: params.tag.clone()})
            },

            _ => self.clone(),
        }
    }

    pub fn to_semver_range(&self) -> Option<zpm_semver::Range> {
        match self {
            Range::AnonymousSemver(params) => {
                Some(params.range.clone())
            },

            Range::RegistrySemver(params) => {
                Some(params.range.clone())
            },

            _ => None,
        }
    }

    pub fn to_peer_range(&self) -> Result<PeerRange, RangeError> {
        match self {
            Range::AnonymousSemver(params) => {
                Ok(PeerRange::Semver(SemverPeerRange {range: params.range.clone()}))
            },

            Range::RegistrySemver(params) => {
                Ok(PeerRange::Semver(SemverPeerRange {range: params.range.clone()}))
            },

            _ => {
                Err(RangeError::PeerRangeError(self.to_file_string()))
            },
        }
    }
}

impl ToFileString for Range {
    fn to_file_string(&self) -> String {
        match self {
            Range::Builtin(params) => {
                format!("builtin:{}", params.range.to_file_string())
            },

            Range::AnonymousSemver(params) => {
                params.range.to_file_string()
            },

            Range::AnonymousTag(params) => {
                params.tag.clone()
            },

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

            Range::Patch(params) => {
                format!("patch:{}#{}", params.inner.to_file_string(), params.path)
            },

            Range::Link(params) => {
                format!("link:{}", params.path)
            },

            Range::Portal(params) => {
                format!("portal:{}", params.path)
            },

            Range::Url(params) => {
                params.url.clone()
            },

            Range::WorkspaceSemver(params) => {
                format!("workspace:{}", params.range.to_file_string())
            },

            Range::WorkspaceMagic(params) => {
                format!("workspace:{}", serde_plain::to_string(&params.magic).unwrap())
            },

            Range::WorkspacePath(params) => {
                format!("workspace:{}", params.path.to_file_string())
            },

            Range::WorkspaceIdent(params) => {
                format!("workspace:{}", params.ident.to_file_string())
            },

            Range::Git(params) => {
                params.git.to_file_string()
            },

            Range::Virtual(params) => {
                format!("virtual:{}#{}", params.inner.to_file_string(), params.hash.to_file_string())
            },

            Range::MissingPeerDependency => {
                "missing!".to_string()
            },
        }
    }
}

impl ToHumanString for Range {
    fn to_print_string(&self) -> String {
        if let Range::Virtual(params) = self {
            format!("{} {}", params.inner.to_print_string(), DataType::Range.colorize(&format!("[{}]", params.hash.mini())))
        } else {
            DataType::Range.colorize(&self.to_file_string())
        }
    }
}

impl_file_string_from_str!(Range);
impl_file_string_serialization!(Range);
