use std::hash::Hash;

use bincode::{Decode, Encode};
use zpm_macros::Parsed;

use crate::{error::Error, git, semver, yarn_serialization_protocol};

use super::{Ident, Locator};

#[derive(Clone, Debug, Decode, Encode, Parsed, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[parse_error(Error::InvalidRange)]
pub enum Range {
    #[try_pattern()]
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

    #[try_pattern(prefix = "file:")]
    File(String),

    #[try_pattern(prefix = "patch:")]
    Patch(String),

    #[try_pattern(prefix = "workspace:")]
    WorkspaceSemver(semver::Range),

    #[try_pattern(prefix = "workspace:", pattern = r"([~^=*])")]
    WorkspaceMagic(String),

    #[try_pattern(prefix = "workspace:")]
    WorkspacePath(String),

    #[try_pattern()]
    Git(git::GitRange),

    MissingPeerDependency,
    Virtual(Box<Range>, u64),
}

impl Range {
    pub fn must_bind(&self) -> bool {
        match &self {
            Range::Link(_) | Range::Portal(_) | Range::File(_) | Range::Patch(_) => true,
            _ => false,
        }
    }
}

yarn_serialization_protocol!(Range, "", {
    serialize(&self) {
        match self {
            Range::Semver(range) => format!("npm:{}", range),
            Range::SemverTag(tag) => format!("npm:{}", tag),
            Range::SemverAlias(ident, range) => format!("npm:{}@{}", ident, range),
            Range::Patch(patch) => format!("patch:{}", patch),
            Range::Link(link) => format!("link:{}", link),
            Range::Portal(portal) => format!("portal:{}", portal),
            Range::File(file) => format!("file:{}", file),
            Range::WorkspaceSemver(semver) => format!("workspace:{}", semver),
            Range::WorkspaceMagic(magic) => format!("workspace:{}", magic),
            Range::WorkspacePath(path) => format!("workspace:{}", path),
            Range::Git(git) => git.to_string(),
            Range::MissingPeerDependency => format!("missing!"),
            Range::Virtual(inner, hash) => format!("{} [{:016x}]", inner, hash),
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
