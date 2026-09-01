use std::{hash::Hash, sync::Arc};

use rkyv::Archive;
use rstest::rstest;
use zpm_utils::{impl_file_string_from_str, impl_file_string_serialization, DataType, FromFileString, Hash64, ToFileString, ToHumanString};

use crate::{IdentError, ReferenceError};

use super::{reference::VirtualReference, split_ident_and_selector, Ident, Reference};

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

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Archive, rkyv::Serialize, rkyv::Deserialize)]
#[rkyv(derive(PartialEq, Eq, PartialOrd, Ord, Hash))]
#[rkyv(serialize_bounds(__S: rkyv::ser::Writer + rkyv::ser::Allocator + rkyv::ser::Sharing, <__S as rkyv::rancor::Fallible>::Error: rkyv::rancor::Source))]
#[rkyv(deserialize_bounds(__D: rkyv::de::Pooling, <__D as rkyv::rancor::Fallible>::Error: rkyv::rancor::Source))]
#[rkyv(bytecheck(bounds(__C: rkyv::validation::ArchiveContext + rkyv::validation::SharedContext, <__C as rkyv::rancor::Fallible>::Error: rkyv::rancor::Source)))]
pub struct Locator {
    pub ident: Ident,
    pub reference: Reference,
    #[rkyv(omit_bounds)]
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
        let physical_reference = self.reference.physical_reference();

        if physical_reference == &self.reference {
            return self.clone();
        }

        Locator::new_bound(self.ident.clone(), physical_reference.clone(), self.parent.clone())
    }

    pub fn virtualized_for(&self, parent: &Locator) -> Locator {
        self.virtualized_with_hash(Hash64::from_string(parent))
    }

    pub fn virtualized_with_hash(&self, hash: Hash64) -> Locator {
        let reference = Reference::Virtual(VirtualReference {
            inner: Box::new(self.reference.clone()),
            hash,
        });

        Locator {
            ident: self.ident.clone(),
            reference,
            parent: self.parent.clone(),
        }
    }

    pub fn env_qualified_with_hash(&self, hash: Hash64) -> Locator {
        Locator {
            ident: self.ident.clone(),
            reference: self.reference.env_qualified_with_hash(hash),
            parent: self.parent.clone(),
        }
    }

    pub fn slug(&self) -> String {
        let key
            = Hash64::from_string(&self.to_file_string());

        format!("{}-{}-{}", self.ident.slug(), self.reference.slug(), key.short())
    }
}

impl FromFileString for Locator {
    type Error = LocatorError;

    fn from_file_string(src: &str) -> Result<Self, Self::Error> {
        let (ident_str, after_at) = split_ident_and_selector(src);
        let after_at = after_at
            .ok_or_else(|| LocatorError::SyntaxError(src.to_string()))?;

        let parent_marker
            = "::parent=";
        let parent_split
            = after_at.find(parent_marker);

        let ident
            = Ident::from_file_string(ident_str)?;
        let reference
            = Reference::from_file_string(&after_at[..parent_split.unwrap_or(after_at.len())])?;

        let parent = parent_split
            .map(|idx| Locator::from_file_string(&after_at[idx + parent_marker.len()..]))
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
#[case("foo@pypi:1.0.0")]
#[case(&format!("foo@env:{}#pypi:1.0.0", Hash64::from_data("fork").to_file_string()))]
#[case("foo@npm:1.0.0::parent=root@workspace:.")]
fn test_locator_serialization(#[case] str: &str) {
    assert_eq!(str, Locator::from_file_string(str).unwrap().to_file_string());
}

#[test]
fn test_env_locator_physical_locator() {
    let hash
        = Hash64::from_data("fork");
    let locator
        = Locator::from_file_string(&format!("foo@env:{}#pypi:1.0.0", hash.to_file_string())).unwrap();

    assert_eq!("foo@pypi:1.0.0", locator.physical_locator().to_file_string());
}

#[test]
fn test_env_locator_preserves_virtual_outer_wrapper() {
    let fork_hash
        = Hash64::from_data("fork");
    let peer_hash
        = Hash64::from_data("peer");
    let locator
        = Locator::from_file_string("foo@pypi:1.0.0").unwrap()
            .virtualized_with_hash(peer_hash.clone())
            .env_qualified_with_hash(fork_hash.clone());

    assert_eq!(
        format!("foo@virtual:{}#env:{}#pypi:1.0.0", peer_hash.to_file_string(), fork_hash.to_file_string()),
        locator.to_file_string(),
    );
}
