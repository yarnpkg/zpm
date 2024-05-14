use std::collections::{BTreeMap, HashMap};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::str::FromStr;

use bincode::{Decode, Encode};
use serde::{Deserialize, Deserializer, Serialize};

use crate::{error::Error, resolver::{resolve, Resolution}, yarn_serialization_protocol};

use super::{Ident, Locator, Range};

#[derive(Clone, Debug, Decode, Encode, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Descriptor {
    pub ident: Ident,
    pub range: Range,
    pub parent: Option<Locator>,
}

impl Descriptor {
    pub fn new(ident: Ident, range: Range) -> Descriptor {
        Descriptor {
            ident,
            range,
            parent: None,
        }
    }

    pub fn new_bound(ident: Ident, range: Range, parent: Option<Locator>) -> Descriptor {
        Descriptor {
            ident,
            range,
            parent,
        }
    }

    pub fn virtualized_for(&self, parent: &Locator) -> Descriptor {
        let mut s = DefaultHasher::new();
        parent.hash(&mut s);

        Descriptor {
            ident: self.ident.clone(),
            range: Range::Virtual(Box::new(self.range.clone()), s.finish()),
            parent: self.parent.clone(),
        }
    }

    pub async fn resolve(self) -> Result<Resolution, Error> {
        resolve(self).await
    }

    pub async fn resolve_with_descriptor(self) -> (Descriptor, Result<Resolution, Error>) {
        let descriptor = self.clone();
        let resolution = resolve(self).await;

        (descriptor, resolution)
    }
}

pub fn descriptor_map_serializer<S>(value: &HashMap<Ident, Descriptor>, serializer: S) -> Result<S::Ok, S::Error> where S: serde::Serializer {
    let mut map = BTreeMap::new();

    for (k, v) in value.iter() {
        map.insert(k.clone(), v.range.clone());
    }

    map.serialize(serializer)
}

pub fn descriptor_map_deserializer<'de, D>(deserializer: D) -> Result<HashMap<Ident, Descriptor>, D::Error> where D: Deserializer<'de> {
    let values = HashMap::<Ident, Range>::deserialize(deserializer)?;
    let mut entries = HashMap::new();

    for (k, v) in values.iter() {
        let descriptor = Descriptor::new(k.clone(), v.clone());
        entries.insert(k.clone(), descriptor);
    }

    Ok(entries)
}

yarn_serialization_protocol!(Descriptor, "", {
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
        let range = Range::from_str(&src[at_split + 1..parent_split.map_or(src.len(), |idx| idx)])?;

        let parent = match parent_split {
            Some(idx) => Some(Locator::from_str(&src[idx + 10..])?),
            None => None,
        };

        Ok(Descriptor::new_bound(ident, range, parent))
    }

    serialize(&self) {
        match &self.parent {
            Some(parent) => format!("{}@{}::parent={}", self.ident, self.range, parent),
            None => format!("{}@{}", self.ident, self.range),
        }
    }
});
