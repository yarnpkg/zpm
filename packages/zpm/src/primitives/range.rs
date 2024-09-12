use std::hash::Hash;

use bincode::{Decode, Encode};
use zpm_macros::Parsed;

use crate::{error::Error, git, hash::Sha256, semver, yarn_serialization_protocol};

use super::Ident;

#[derive(Clone, Debug, Decode, Encode, Parsed, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[parse_error(Error::InvalidRange)]
pub enum Range {
    #[try_pattern(pattern = r"missing!")]
    MissingPeerDependency(),

    #[try_pattern()]
    SemverOrWorkspace(semver::Range),

    #[try_pattern(prefix = "npm:")]
    Semver(semver::Range),

    #[try_pattern(prefix = "npm:", pattern = r"([-a-z0-9._^v][-a-z0-9._]*)", optional_prefix = true)]
    SemverTag(String),

    #[try_pattern(prefix = "npm:", pattern = r"(.*)@(.*)")]
    SemverAlias(Ident, semver::Range),

    #[try_pattern(prefix = "link:")]
    Link(String),

    #[try_pattern(prefix = "portal:")]
    Portal(String),

    #[try_pattern(prefix = "file:", pattern = r"(.*\.(?:tgz|tar\.gz))")]
    #[try_pattern(pattern = r"(\.{0,2}/.*\.(?:tgz|tar\.gz))")]
    Tarball(String),

    #[try_pattern(prefix = "file:")]
    #[try_pattern(pattern = r"(\.{0,2}/.*)")]
    Folder(String),

    #[try_pattern(prefix = "patch:")]
    Patch(String),

    #[try_pattern(prefix = "virtual:", pattern = r"(.*)#([a-f0-9]*)$")]
    Virtual(Box<Range>, Sha256),

    #[try_pattern(prefix = "workspace:")]
    WorkspaceSemver(semver::Range),

    #[try_pattern(prefix = "workspace:", pattern = r"([~^=*])")]
    WorkspaceMagic(String),

    #[try_pattern(prefix = "workspace:")]
    WorkspacePath(String),

    #[try_pattern()]
    Git(git::GitRange),

    #[try_pattern(pattern = r"(https?://.*(?:/.*|\.tgz|\.tar\.gz))")]
    Url(String),
}

impl Range {
    pub fn must_bind(&self) -> bool {
        matches!(&self, Range::Link(_) | Range::Portal(_) | Range::Tarball(_) | Range::Folder(_) | Range::Patch(_))
    }

    pub fn is_transient_resolution(&self) -> bool {
        matches!(&self, Range::Link(_) | Range::Portal(_) | Range::Tarball(_) | Range::Folder(_) | Range::Patch(_) | Range::WorkspaceMagic(_) | Range::WorkspacePath(_) | Range::WorkspaceSemver(_))
    }
}

yarn_serialization_protocol!(Range, "", {
    serialize(&self) {
        match self {
            Range::SemverOrWorkspace(range) => range.to_string(),
            Range::Semver(range) => format!("npm:{}", range),
            Range::SemverTag(tag) => format!("npm:{}", tag),
            Range::SemverAlias(ident, range) => format!("npm:{}@{}", ident, range),
            Range::Patch(patch) => format!("patch:{}", patch),
            Range::Link(link) => format!("link:{}", link),
            Range::Portal(portal) => format!("portal:{}", portal),
            Range::Tarball(file) => format!("file:{}", file),
            Range::Folder(file) => format!("file:{}", file),
            Range::Url(url) => url.to_string(),
            Range::WorkspaceSemver(semver) => format!("workspace:{}", semver),
            Range::WorkspaceMagic(magic) => format!("workspace:{}", magic),
            Range::WorkspacePath(path) => format!("workspace:{}", path),
            Range::Git(git) => git.to_string(),
            Range::MissingPeerDependency() => "missing!".to_string(),
            Range::Virtual(inner, hash) => format!("virtual:{}#{}", inner, hash),
        }
    }
});

#[derive(Clone, Debug, Decode, Encode, Parsed, PartialEq, Eq, Hash)]
#[parse_error(Error::InvalidRange)]
pub enum PeerRange {
    #[try_pattern()]
    Semver(semver::Range),

    #[try_pattern(prefix = "workspace:")]
    WorkspaceSemver(String),

    #[try_pattern(prefix = "workspace:", pattern = r"^([~^=*])$")]
    WorkspaceMagic(String),

    #[try_pattern(prefux = "workspace:")]
    WorkspacePath(String),
}

yarn_serialization_protocol!(PeerRange, "", {
    serialize(&self) {
        match self {
            PeerRange::Semver(range) => range.to_string(),
            PeerRange::WorkspaceSemver(semver) => format!("workspace:{}", semver),
            PeerRange::WorkspaceMagic(magic) => format!("workspace:{}", magic),
            PeerRange::WorkspacePath(path) => format!("workspace:{}", path),
        }
    }
});
