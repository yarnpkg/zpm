use std::{hash::Hash, sync::Arc};

use bincode::{Decode, Encode};
use rstest::rstest;
use zpm_utils::{impl_file_string_from_str, impl_file_string_serialization, DataType, FromFileString, Hash64, ToFileString, ToHumanString};

use crate::{IdentError, ReferenceError};

use super::{reference::VirtualReference, Ident, Reference};

#[derive(thiserror::Error, Clone, Debug)]
pub enum LocatorError {
    #[error("Invalid locator: {0}")]
    SyntaxError(String),

    #[error(transparent)]
    FromUtf8Error(#[from] std::string::FromUtf8Error),

    #[error(transparent)]
    IdentError(#[from] IdentError),

    #[error(transparent)]
    ReferenceError(#[from] ReferenceError),

    #[error(transparent)]
    ParentError(#[from] Arc<LocatorError>),
}

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
            hash: Hash64::from_string(&serialized),
        });

        Locator {
            ident: self.ident.clone(),
            reference,
            parent: self.parent.clone(),
        }
    }

    pub fn slug(&self) -> String {
        let key
            = Hash64::from_string(&self.to_file_string());

        format!("{}-{}-{}", self.ident.slug(), self.reference.slug(), key.to_file_string())
    }
}

impl FromFileString for Locator {
    type Error = LocatorError;

    fn from_file_string(src: &str) -> Result<Self, Self::Error> {
        let at_split = src.strip_prefix('@')
            .map_or_else(|| src.find('@'), |rest| rest.find('@').map(|x| x + 1))
            .ok_or_else(|| LocatorError::SyntaxError(src.to_string()))?;

        let parent_marker
            = "::parent=";
        let parent_split
            = src.find(parent_marker);

        let ident
            = Ident::from_file_string(&src[..at_split])?;
        let reference
            = Reference::from_file_string(&src[at_split + 1..parent_split.map_or(src.len(), |idx| idx)])?;

        let parent = parent_split
            .map(|idx| Locator::from_file_string(&src[idx + parent_marker.len()..]))
            .transpose()?
            .map(Arc::new);

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
        final_str.push_str(&DataType::Custom(135, 175, 255).colorize("@"));
        final_str.push_str(&serialized_reference);

        if let Some(parent) = &self.parent {
            final_str.push_str("::parent=");
            final_str.push_str(&parent.to_print_string());
        }

        final_str
    }
}

impl_file_string_from_str!(Locator);
impl_file_string_serialization!(Locator);

#[rstest]
#[case("foo@npm:1.0.0")]
#[case("foo@npm:1.0.0::parent=root@workspace:")]
fn test_locator_serialization(#[case] str: &str) {
    assert_eq!(str, Locator::from_file_string(str).unwrap().to_file_string());
}
