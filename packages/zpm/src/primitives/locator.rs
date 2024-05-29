use std::{hash::Hash, str::FromStr, sync::Arc};

use bincode::{Decode, Encode};
use sha2::Digest;
use zpm_macros::Parsed;

use crate::{error::Error, hash::Sha256, serialize::Serialized, yarn_serialization_protocol};

use super::{Ident, Reference};

#[derive(Clone, Debug, Parsed, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[parse_error(Error::InvalidIdentOrLocator)]
pub enum IdentOrLocator {
    #[try_pattern(pattern = "(@?[^@]+)")]
    Ident(Ident),

    #[try_pattern()]
    Locator(Locator),
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
        match &self.reference {
            Reference::Virtual(inner, _) => Locator::new_bound(self.ident.clone(), inner.physical_reference(), self.parent.clone()),
            _ => self.clone(),
        }
    }

    pub fn virtualized_for(&self, parent: &Locator) -> Locator {
        let serialized = parent.serialized()
            .expect(format!("Failed to serialize locator: {:?}", self).as_str());

        Locator {
            ident: self.ident.clone(),
            reference: Reference::Virtual(Box::new(self.reference.clone()), Sha256::from_string(&serialized)),
            parent: self.parent.clone(),
        }
    }

    pub fn slug(&self) -> String {
        let mut key = sha2::Sha256::new();
        key.update(self.to_string());
        let key = key.finalize();

        format!("{}-{}-{:064x}", self.ident.slug(), self.reference.slug(), key)
    }
}

yarn_serialization_protocol!(Locator, "", {
    deserialize(src) {
        let at_split = if src.starts_with('@') {
            src[1..src.len()].find('@').map(|x| x + 1)
        } else {
            src.find('@')
        };

        let at_split = at_split
            .ok_or(Error::InvalidDescriptor(src.to_string()))?;

        let parent_split = src.find("::parent=");

        let ident = Ident::from_str(&src[..at_split])?;
        let reference = Reference::from_str(&src[at_split + 1..parent_split.map_or(src.len(), |idx| idx)])?;

        let parent = match parent_split {
            Some(idx) => Some(Arc::new(Locator::from_str(&src[idx + 10..])?)),
            None => None,
        };

        Ok(Locator::new_bound(ident, reference, parent))
    }

    serialize(&self) {
        match &self.parent {
            Some(parent) => format!("{}@{}::parent={}", self.ident, self.reference, parent),
            None => format!("{}@{}", self.ident, self.reference),
        }
    }
});
