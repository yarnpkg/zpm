use std::collections::{BTreeMap, HashMap};
use std::hash::Hash;
use std::str::FromStr;

use bincode::{Decode, Encode};
use rstest::rstest;
use serde::{Deserialize, Deserializer, Serialize};

use crate::hash::Sha256;
use crate::serialize::Serialized;
use crate::{semver, yarn_check_serialize};
use crate::{error::Error, yarn_serialization_protocol};

use super::{Ident, Locator, Range};

#[derive(Debug)]
pub struct LooseDescriptor {
    pub descriptor: Descriptor,
}

impl<'a> FromStr for LooseDescriptor {
    type Err = crate::error::Error;
    
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Ok(descriptor) = Descriptor::from_str(s) {
            return Ok(LooseDescriptor {descriptor});
        }

        let ident = Ident::from_str(s)?;
        let range = Range::from_str("latest")?;

        let descriptor = Descriptor::new(ident, range);

        Ok(LooseDescriptor {descriptor})
    }
}

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

    pub fn new_semver(ident: Ident, range: &str) -> Result<Descriptor, Error> {
        Ok(Descriptor {
            ident,
            range: Range::Semver(semver::Range::from_str(range)?),
            parent: None,
        })
    }

    pub fn new_bound(ident: Ident, range: Range, parent: Option<Locator>) -> Descriptor {
        Descriptor {
            ident,
            range,
            parent,
        }
    }

    pub fn virtualized_for(&self, parent: &Locator) -> Descriptor {
        let serialized = parent.serialized()
            .unwrap_or_else(|_| panic!("Failed to serialize locator: {:?}", self));

        Descriptor {
            ident: self.ident.clone(),
            range: Range::Virtual(Box::new(self.range.clone()), Sha256::from_string(&serialized)),
            parent: self.parent.clone(),
        }
    }
}

pub fn descriptor_map_serializer<S>(value: &HashMap<Ident, Descriptor>, serializer: S) -> Result<S::Ok, S::Error> where S: serde::Serializer {
    let mut map = BTreeMap::new();

    for v in value.values() {
        let serialized_range = v.to_string();

        let at_split = match serialized_range.starts_with('@') {
            true => serialized_range[1..serialized_range.len()].find('@').map(|x| x + 1),
            false => serialized_range.find('@'),
        }.unwrap();

        let ident_str = &serialized_range[0..at_split];
        let range_str = &serialized_range[at_split + 1..];

        map.insert(ident_str.to_string(), range_str.to_string());
    }

    map.serialize(serializer)
}

pub fn descriptor_map_deserializer<'de, D>(deserializer: D) -> Result<HashMap<Ident, Descriptor>, D::Error> where D: Deserializer<'de> {
    let values = HashMap::<String, String>::deserialize(deserializer)?;
    let mut entries = HashMap::new();

    for (k, v) in values.iter() {
        let serialized_range = format!("{}@{}", k, v);
        let descriptor = Descriptor::from_str(&serialized_range)
            .map_err(serde::de::Error::custom)?;

        entries.insert(descriptor.ident.clone(), descriptor);
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

        let parent_marker = "::parent=";
        let parent_split = src.find(parent_marker);
    
        let ident = Ident::from_str(&src[..at_split])?;
        let range = Range::from_str(&src[at_split + 1..parent_split.map_or(src.len(), |idx| idx)])?;

        let parent = match parent_split {
            Some(idx) => Some(Locator::from_str(&src[idx + parent_marker.len()..])?),
            None => None,
        };

        Ok(Descriptor::new_bound(ident, range, parent))
    }

    serialize(&self) {
        yarn_check_serialize!(self, match &self.parent {
            Some(parent) => format!("{}@{}::parent={}", self.ident, self.range, parent),
            None => format!("{}@{}", self.ident, self.range),
        })
    }
});

#[rstest]
#[case("foo@npm:1.0.0")]
#[case("foo@npm:1.0.0::parent=root@workspace:")]
fn test_descriptor_serialization(#[case] str: &str) {
    assert_eq!(str, Descriptor::from_str(str).unwrap().to_string());
}
