use bincode::{Decode, Encode};
use colored::Colorize;
use zpm_utils::{impl_serialization_traits, FromFileString, ToFileString, ToHumanString};

use crate::error::Error;
use super::Ident;

#[derive(Clone, Debug, Decode, Encode, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SemverDescriptor {
    pub ident: Ident,
    pub range: zpm_semver::Range,
}

impl SemverDescriptor {
    pub fn new(ident: Ident, range: zpm_semver::Range) -> SemverDescriptor {
        SemverDescriptor {
            ident,
            range,
        }
    }
}

impl FromFileString for SemverDescriptor {
    type Error = Error;

    fn from_file_string(src: &str) -> Result<Self, Error> {
        let at_split = if src.starts_with('@') {
            src[1..src.len()].find('@').map(|x| x + 1)
        } else {
            src.find('@')
        };

        let at_split = at_split
            .ok_or(Error::InvalidDescriptor(src.to_string()))?;

        let ident = Ident::from_file_string(&src[..at_split])?;
        let range = zpm_semver::Range::from_file_string(&src[at_split + 1..])?;

        Ok(SemverDescriptor::new(ident, range))
    }
}

impl ToFileString for SemverDescriptor {
    fn to_file_string(&self) -> String {
        format!("{}@{}", self.ident.to_file_string(), self.range.to_file_string())
    }
}

impl ToHumanString for SemverDescriptor {
    fn to_print_string(&self) -> String {
        format!("{}{}{}",
            self.ident.to_print_string(),
            "@".truecolor(0, 175, 175),
            self.range.to_print_string()
        )
    }
}

impl_serialization_traits!(SemverDescriptor);
