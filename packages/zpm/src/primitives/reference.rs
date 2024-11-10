use std::hash::Hash;

use bincode::{Decode, Encode};
use zpm_macros::parse_enum;

use crate::{error::Error, git, hash::Sha256, semver, serialize::UrlEncoded, yarn_check_serialize, yarn_serialization_protocol};

use super::{Ident, Locator};

#[parse_enum(or_else = |s| Err(Error::InvalidReference(s.to_string())))]
#[derive(Clone, Debug, Decode, Encode, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[derive_variants(Clone, Debug, Decode, Encode, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Reference {
    #[pattern(spec = r"npm:(?<version>.*)")]
    Shorthand {
        version: semver::Version,
    },

    #[pattern(spec = r"npm:(?<ident>.*)@(?<version>.*)")]
    Registry {
        ident: Ident,
        version: semver::Version,
    },

    #[pattern(spec = r"file:(?<path>.*\.(?:tgz|tar\.gz))")]
    Tarball {
        path: String,
    },

    #[pattern(spec = r"file:(?<path>.*)")]
    Folder {
        path: String,
    },

    #[pattern(spec = r"link:(?<path>.*)")]
    Link {
        path: String,
    },

    #[pattern(spec = r"portal:(?<path>.*)")]
    Portal {
        path: String,
    },

    #[pattern(spec = r"patch:(?<inner>.*)#(?<path>.*)$")]
    Patch {
        inner: Box<UrlEncoded<Locator>>,
        path: String,
    },

    #[pattern(spec = r"virtual:(?<inner>.*)#(?<hash>[a-f0-9]*)$")]
    Virtual {
        inner: Box<Reference>,
        hash: Sha256,
    },

    #[pattern(spec = "workspace:(?<ident>.*)")]
    Workspace {
        ident: Ident,
    },

    #[pattern(spec = "git:(?<git>.*)")]
    Git {
        git: git::GitReference,
    },

    #[pattern(spec = r"(?<url>https?://.*(?:/.*|\.tgz|\.tar\.gz))")]
    Url {
        url: String,
    },
}

impl Reference {
    pub fn must_bind(&self) -> bool {
        // Keep this list in sync w/ Range::must_bind
        matches!(&self, Reference::Link(_) | Reference::Portal(_) | Reference::Tarball(_) | Reference::Folder(_) | Reference::Patch(_))
    }

    pub fn physical_reference(&self) -> Reference {
        match self {
            Reference::Virtual(params) => params.inner.physical_reference(),
            _ => self.clone(),
        }
    }

    pub fn slug(&self) -> String {
        match self {
            Reference::Shorthand(params) => format!("npm-{}", params.version),
            Reference::Git(_) => "git".to_string(),
            Reference::Registry(params) => format!("npm-{}", params.version),
            Reference::Tarball(_) => "file".to_string(),
            Reference::Folder(_) => "file".to_string(),
            Reference::Link(_) => "link".to_string(),
            Reference::Patch(_) => "patch".to_string(),
            Reference::Portal(_) => "portal".to_string(),
            Reference::Url(_) => "url".to_string(),
            Reference::Virtual(_) => "virtual".to_string(),
            Reference::Workspace(_) => "workspace".to_string(),
        }
    }
}

yarn_serialization_protocol!(Reference, "", {
    serialize(&self) {
        yarn_check_serialize!(self, match self {
            Reference::Shorthand(params) => format!("npm:{}", params.version),
            Reference::Git(params) => format!("git:{}", params.git),
            Reference::Registry(params) => format!("npm:{}@{}", params.ident, params.version),
            Reference::Tarball(params) => format!("file:{}", params.path),
            Reference::Folder(params) => format!("file:{}", params.path),
            Reference::Link(params) => format!("link:{}", params.path),
            Reference::Patch(params) => format!("patch:{}#{}", params.inner, params.path),
            Reference::Portal(params) => format!("portal:{}", params.path),
            Reference::Url(params) => params.url.to_string(),
            Reference::Virtual(params) => format!("virtual:{}#{}", params.inner, params.hash),
            Reference::Workspace(params) => format!("workspace:{}", params.ident),
        })
    }
});
