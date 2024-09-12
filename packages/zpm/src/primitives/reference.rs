use std::hash::Hash;

use bincode::{Decode, Encode};
use zpm_macros::Parsed;

use crate::{error::Error, hash::Sha256, semver, yarn_serialization_protocol};

use super::Ident;

#[derive(Clone, Debug, Decode, Encode, Parsed, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[parse_error(Error::InvalidReference)]
pub enum Reference {
    #[try_pattern()]
    #[try_pattern(prefix = "npm:")]
    Semver(semver::Version),

    #[try_pattern(prefix = "npm:", pattern = r"^(.*)@(.*)$")]
    SemverAlias(Ident, semver::Version),

    #[try_pattern(prefix = "file:", pattern = r"(.*\.(?:tgz|tar\.gz))")]
    Tarball(String),

    #[try_pattern(prefix = "file:")]
    Folder(String),

    #[try_pattern(prefix = "link:")]
    Link(String),

    #[try_pattern(prefix = "portal:")]
    Portal(String),

    #[try_pattern(prefix = "virtual:", pattern = r"(.*)#([a-f0-9]*)$")]
    Virtual(Box<Reference>, Sha256),

    #[try_pattern(prefix = "workspace:")]
    Workspace(Ident),

    #[try_pattern(prefix = "git:", pattern = r"(.*)#(.*)")]
    Git(String, String),

    #[try_pattern(pattern = r"(https?://.*(?:/.*|\.tgz|\.tar\.gz))")]
    Url(String),
}

impl Reference {
    pub fn physical_reference(&self) -> Reference {
        match self {
            Reference::Virtual(inner, _) => inner.physical_reference(),
            _ => self.clone(),
        }
    }

    pub fn slug(&self) -> String {
        match self {
            Reference::Git(_, _) => "git".to_string(),
            Reference::Semver(version) => format!("npm-{}", version),
            Reference::SemverAlias(_, version) => format!("npm-{}", version),
            Reference::Tarball(_) => "file".to_string(),
            Reference::Folder(_) => "file".to_string(),
            Reference::Link(_) => "link".to_string(),
            Reference::Portal(_) => "portal".to_string(),
            Reference::Url(_) => "url".to_string(),
            Reference::Virtual(_, _) => "virtual".to_string(),
            Reference::Workspace(_) => "workspace".to_string(),
        }
    }
}

yarn_serialization_protocol!(Reference, "", {
    serialize(&self) {
        match self {
            Reference::Git(repo, commit) => format!("git:{}#{}", repo, commit),
            Reference::Semver(version) => format!("npm:{}", version),
            Reference::SemverAlias(ident, version) => format!("npm:{}@{}", ident, version),
            Reference::Tarball(path) => format!("file:{}", path),
            Reference::Folder(path) => format!("file:{}", path),
            Reference::Link(path) => format!("link:{}", path),
            Reference::Portal(path) => format!("portal:{}", path),
            Reference::Url(url) => url.to_string(),
            Reference::Virtual(inner, hash) => format!("virtual:{}#{}", inner, hash),
            Reference::Workspace(ident) => format!("workspace:{}", ident),
        }
    }
});
