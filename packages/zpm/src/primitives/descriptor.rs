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
}

impl Descriptor {
    pub fn new(ident: Ident, range: Range) -> Descriptor {
        Descriptor {
            ident,
            range,
        }
    }

    pub fn virtualized_for(&self, parent: &Locator) -> Descriptor {
        let mut s = DefaultHasher::new();
        parent.hash(&mut s);

        Descriptor {
            ident: self.ident.clone(),
            range: Range::Virtual(Box::new(self.range.clone()), s.finish()),
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
        let split_point = if src.starts_with('@') {
            src[1..src.len()].find('@').map(|x| x + 1)
        } else {
            src.find('@')
        };

        let split_point = split_point
            .ok_or(Error::InvalidDescriptor(src.to_string()))?;

        let ident = Ident::from_str(&src[..split_point])?;
        let range = Range::from_str(&src[split_point + 1..])?;

        Ok(Descriptor::new(ident, range))
    }

    serialize(&self) {
        format!("{}@{}", self.ident, self.range)
    }
});
