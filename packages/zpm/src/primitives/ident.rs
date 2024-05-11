use std::hash::Hash;

use bincode::{Decode, Encode};

use crate::yarn_serialization_protocol;

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
}

impl AsRef<str> for Ident {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

yarn_serialization_protocol!(Ident, "", {
    deserialize(src) {
        Ok(Ident::new(src))
    }

    serialize(&self) {
        self.as_str()
    }
});
