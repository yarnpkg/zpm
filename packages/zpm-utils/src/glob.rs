use ouroboros::self_referencing;
use wax::Program;

use crate::{FromFileString, Path, ToFileString, ToHumanString, impl_file_string_from_str, impl_file_string_serialization};

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

    pub fn prefix(&self) -> Result<Path, crate::PathError> {
        Path::try_from(self.inner.borrow_pattern().clone().partition().0)
    }

    pub fn raw(&self) -> &str {
        self.inner.borrow_raw()
    }

    pub fn matcher(&self) -> &wax::Glob<'_> {
        self.inner.borrow_pattern()
    }

    pub fn is_match(&self, s: &str) -> bool {
        self.matcher().is_match(s)
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
    fn write_file_string<W: std::fmt::Write>(&self, out: &mut W) -> std::fmt::Result {
        out.write_str(self.raw())
    }
}

impl ToHumanString for Glob {
    fn to_print_string(&self) -> String {
        let mut buffer = String::new();
        let _ = self.write_file_string(&mut buffer);
        buffer
    }
}

impl_file_string_from_str!(Glob);
impl_file_string_serialization!(Glob);
