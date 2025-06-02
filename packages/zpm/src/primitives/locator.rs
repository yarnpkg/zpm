use std::{hash::Hash, sync::Arc};

use bincode::{Decode, Encode};
use colored::Colorize;
use rstest::rstest;
use sha2::Digest;
use zpm_utils::{impl_serialization_traits, FromFileString, ToFileString, ToHumanString};

use crate::{error::Error, hash::Sha256};

use super::{reference::VirtualReference, Ident, Reference};

#[derive(Clone, Debug, Decode, Encode, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Locator {
    pub ident: Ident,
    pub reference: Reference,
    pub parent: Option<Arc<Locator>>,
}

impl Locator {
    pub fn new(ident: Ident, reference: Reference) -> Locator {
        Locator {
            ident,
            reference,
            parent: None,
        }
    }

    pub fn new_bound(ident: Ident, reference: Reference, parent: Option<Arc<Locator>>) -> Locator {
        Locator {
            ident,
            reference,
            parent,
        }
    }

    pub fn physical_locator(&self) -> Locator {
        if let Reference::Virtual(params) = &self.reference {
            Locator::new_bound(self.ident.clone(), params.inner.physical_reference().clone(), self.parent.clone())
        } else {
            self.clone()
        }
    }

    pub fn virtualized_for(&self, parent: &Locator) -> Locator {
        let serialized = parent.to_file_string();

        let reference = Reference::Virtual(VirtualReference {
            inner: Box::new(self.reference.clone()),
            hash: Sha256::from_string(&serialized),
        });

        Locator {
            ident: self.ident.clone(),
            reference,
            parent: self.parent.clone(),
        }
    }

    pub fn slug(&self) -> String {
        let mut key = sha2::Sha256::new();
        key.update(self.to_file_string());
        let key = key.finalize();

        format!("{}-{}-{:064x}", self.ident.slug(), self.reference.slug(), key)
    }
}

impl FromFileString for Locator {
    type Error = Error;

    fn from_file_string(src: &str) -> Result<Self, Self::Error> {
        let at_split = if src.starts_with('@') {
            src[1..src.len()].find('@').map(|x| x + 1)
        } else {
            src.find('@')
        };

        let at_split = at_split
            .ok_or(Error::InvalidDescriptor(src.to_string()))?;

        let parent_marker = "::parent=";
        let parent_split = src.find(parent_marker);

        let ident = Ident::from_file_string(&src[..at_split])?;
        let reference = Reference::from_file_string(&src[at_split + 1..parent_split.map_or(src.len(), |idx| idx)])?;

        let parent = match parent_split {
            Some(idx) => Some(Arc::new(Locator::from_file_string(&src[idx + parent_marker.len()..])?)),
            None => None,
        };

        Ok(Locator::new_bound(ident, reference, parent))
    }
}

impl ToFileString for Locator {
    fn to_file_string(&self) -> String {
        let serialized_ident = self.ident.to_file_string();
        let serialized_reference = self.reference.to_file_string();

        let mut final_str = String::new();
        final_str.push_str(&serialized_ident);
        final_str.push('@');
        final_str.push_str(&serialized_reference);

        if let Some(parent) = &self.parent {
            final_str.push_str("::parent=");
            final_str.push_str(&parent.to_file_string());
        }

        final_str
    }
}

impl ToHumanString for Locator {
    fn to_print_string(&self) -> String {
        let serialized_ident = self.ident.to_print_string();
        let serialized_reference = self.reference.to_print_string();

        let mut final_str = String::new();
        final_str.push_str(&serialized_ident);
        final_str.push_str(&"@".truecolor(135, 175, 255).to_string());
        final_str.push_str(&serialized_reference);

        if let Some(parent) = &self.parent {
            final_str.push_str("::parent=");
            final_str.push_str(&parent.to_print_string());
        }

        final_str
    }
}

impl_serialization_traits!(Locator);

#[rstest]
#[case("foo@npm:1.0.0")]
#[case("foo@npm:1.0.0::parent=root@workspace:")]
fn test_locator_serialization(#[case] str: &str) {
    assert_eq!(str, Locator::from_file_string(str).unwrap().to_file_string());
}
