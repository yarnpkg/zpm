use std::{hash::Hash, sync::LazyLock};

use bincode::{Decode, Encode};

use crate::{error::Error, yarn_check_serialize, yarn_serialization_protocol};

#[derive(Clone, Debug, Default, Decode, Encode, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Ident(String);

impl Ident {
    pub fn new<P: AsRef<str>>(full: P) -> Ident {
        Ident(full.as_ref().to_string())
    }

    pub fn scope(&self) -> Option<&str> {
        self.0.split_once('/').map(|(scope, _)| scope)
    }

    pub fn name(&self) -> &str {
        self.0.split_once('/').map(|(_,  name)| name).unwrap_or(&self.0)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn slug(&self) -> String {
        self.0.replace("/", "-")
    }

    pub fn nm_subdir(&self) -> String {
        format!("node_modules/{}", self.0)
    }

    pub fn type_ident(&self) -> Ident {
        match self.scope() {
            Some(scope) => Ident::new(format!("@types/{}__{}", &scope[1..], self.name())),
            None => Ident::new(format!("@types/{}", self.name())),
        }
    }
}

impl AsRef<str> for Ident {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

static IDENT_REGEX: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"^(?:@[^/]*/)?([^@/]+)$").unwrap()
});

yarn_serialization_protocol!(Ident, "", {
    deserialize(src) {
        if !IDENT_REGEX.is_match(src) {
            return Err(Error::InvalidIdent(src.to_string()));
        }

        Ok(Ident::new(src))
    }

    serialize(&self) {
        yarn_check_serialize!(self, self.as_str())
    }
});
