use ouroboros::self_referencing;
use serde::{de, Deserialize, Deserializer};

use crate::{FromFileString, ToFileString, ToHumanString};

#[self_referencing]
#[derive(Debug)]
struct OwnedGlob {
    raw: String,

    #[borrows(raw)]
    #[covariant]
    pattern: wax::Glob<'this>,
}

impl PartialEq for OwnedGlob {
    fn eq(&self, other: &Self) -> bool {
        self.borrow_raw() == other.borrow_raw()
    }
}

impl Eq for OwnedGlob {}

impl PartialOrd for OwnedGlob {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.borrow_raw().partial_cmp(other.borrow_raw())
    }
}

impl Ord for OwnedGlob {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.borrow_raw().cmp(other.borrow_raw())
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Glob {
    inner: OwnedGlob,
}

impl Clone for Glob {
    fn clone(&self) -> Self {
        Self::parse(self.inner.borrow_raw().clone()).unwrap()
    }
}

impl Glob {
    pub fn parse(raw: impl Into<String>) -> Result<Self, wax::BuildError> {
        let raw = raw.into();

        let pattern = OwnedGlobTryBuilder {
            raw,
            pattern_builder: |raw| wax::Glob::new(raw),
        }.try_build()?;

        Ok(Glob { inner: pattern })
    }

    pub fn raw(&self) -> &str {
        self.inner.borrow_raw()
    }

    pub fn matcher(&self) -> &wax::Glob {
        self.inner.borrow_pattern()
    }

    pub fn to_regex_string(&self) -> String {
        self.matcher()
            .to_regex()
            .to_string()
    }
}

impl FromFileString for Glob {
    type Error = wax::BuildError;

    fn from_file_string(raw: &str) -> Result<Self, Self::Error> {
        Ok(Glob::parse(raw)?)
    }
}

impl ToFileString for Glob {
    fn to_file_string(&self) -> String {
        self.raw().to_string()
    }
}

impl ToHumanString for Glob {
    fn to_print_string(&self) -> String {
        self.to_file_string()
    }
}

impl<'de> Deserialize<'de> for Glob {
    fn deserialize<D>(deserializer: D) -> Result<Glob, D::Error> where D: Deserializer<'de> {
        Ok(Glob::parse(String::deserialize(deserializer)?)
            .map_err(|err| de::Error::custom(err.to_string()))?)
    }
}
