use std::hash::Hash;

use bincode::{Decode, Encode};
use zpm_macros::Parsed;

use crate::{error::Error, git, hash::Sha256, semver, yarn_serialization_protocol};

use super::Ident;

#[derive(Clone, Debug, Decode, Encode, Parsed, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[parse_error(Error::InvalidReference)]
pub enum Reference {
    #[try_pattern(prefix = "npm:")]
    Semver(semver::Version),

    #[try_pattern(prefix = "npm:", pattern = r"^(.*)@(.*)$")]
    SemverAlias(Ident, semver::Version),

    #[try_pattern(prefix = "link:")]
    Link(String),

    #[try_pattern(prefix = "virtual:", pattern = r"(.*)#(.*)$")]
    Virtual(Box<Reference>, Sha256),

    #[try_pattern(prefix = "workspace:")]
    Workspace(Ident),

    #[try_pattern()]
    Git(git::GitRange),
}

impl Reference {
    pub fn physical_reference(&self) -> Reference {
        match self {
            Reference::Virtual(inner, _) => inner.physical_reference(),
            _ => self.clone(),
        }
    }

}

yarn_serialization_protocol!(Reference, "", {
    serialize(&self) {
        match self {
            Reference::Git(range) => range.to_string(),
            Reference::Semver(version) => format!("npm:{}", version),
            Reference::SemverAlias(ident, version) => format!("npm:{}@{}", ident, version),
            Reference::Link(path) => format!("link:{}", path),
            Reference::Virtual(inner, hash) => format!("{} [{}]", inner, hash.short()),
            Reference::Workspace(ident) => format!("workspace:{}", ident),
        }
    }
});
