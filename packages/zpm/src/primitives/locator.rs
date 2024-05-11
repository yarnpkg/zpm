use std::{hash::Hash, str::FromStr};

use bincode::{Decode, Encode};

use crate::{error::Error, fetcher::{fetch, PackageData}, hash::Sha256, serialize::Serialized, yarn_serialization_protocol};

use super::{Ident, Reference};

#[derive(Clone, Debug, Decode, Encode, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Locator {
    pub ident: Ident,
    pub reference: Reference,
}

impl Locator {
    pub fn new(ident: Ident, reference: Reference) -> Locator {
        Locator {
            ident,
            reference,
        }
    }

    pub fn physical_locator(&self) -> Locator {
        match &self.reference {
            Reference::Virtual(inner, _) => Locator::new(self.ident.clone(), inner.physical_reference()),
            _ => self.clone(),
        }
    }

    pub fn virtualized_for(&self, parent: &Locator) -> Locator {
        let serialized = parent.serialized()
            .expect(format!("Failed to serialize locator: {:?}", self).as_str());

        Locator {
            ident: self.ident.clone(),
            reference: Reference::Virtual(Box::new(self.reference.clone()), Sha256::from_string(&serialized)),
        }
    }

    pub async fn fetch(&self) -> Result<PackageData, Error> {
        fetch(self).await
    }
}

yarn_serialization_protocol!(Locator, "", {
    deserialize(src) {
        let split_point = if src.starts_with('@') {
            src[1..src.len()].find('@').map(|x| x + 1)
        } else {
            src.find('@')
        };

        let ident = Ident::from_str(&src[..split_point.unwrap()])?;
        let range = Reference::from_str(&src[split_point.unwrap() + 1..])?;

        Ok(Locator::new(ident, range))
    }

    serialize(&self) {
        format!("{}@{}", self.ident, self.reference)
    }
});
