use std::collections::BTreeMap;
use std::fmt;
use std::hash::Hash;
use std::str::FromStr;
use std::sync::Arc;

use bincode::{Decode, Encode};
use colored::Colorize;
use rstest::rstest;
use serde::de::{MapAccess, Visitor};
use serde::ser::SerializeMap;
use serde::Deserializer;
use zpm_utils::{impl_serialization_traits, FromFileString, ToFileString, ToHumanString};

use crate::hash::Sha256;
use crate::error::Error;

use super::range::{AnonymousSemverRange, VirtualRange};
use super::{reference, Ident, Locator, Range, Reference};

#[derive(Debug)]
pub struct LooseDescriptor {
    pub descriptor: Descriptor,
}

impl FromStr for LooseDescriptor {
    type Err = crate::error::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        LooseDescriptor::from_file_string(s)
    }
}

impl FromFileString for LooseDescriptor {
    type Error = crate::error::Error;
    
    fn from_file_string(s: &str) -> Result<Self, Self::Error> {
        if let Ok(descriptor) = Descriptor::from_file_string(s) {
            return Ok(LooseDescriptor {descriptor});
        }

        let ident = Ident::from_file_string(s)?;
        let range = Range::from_file_string("latest")?;

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

    pub fn new_semver(ident: Ident, range_str: &str) -> Result<Descriptor, Error> {
        let range = Range::AnonymousSemver(AnonymousSemverRange {
            range: zpm_semver::Range::from_str(range_str)?,
        });

        Ok(Descriptor {
            ident,
            range,
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

    pub fn resolve_with(&self, reference: Reference) -> Locator {
        let parent = match reference.must_bind() {
            true => self.parent.clone().map(Arc::new),
            false => None,
        };

        let reference = match reference {
            Reference::Registry(params) if params.ident == self.ident => reference::ShorthandReference {
                version: params.version,
            }.into(),

            _ => reference,
        };

        Locator::new_bound(self.ident.clone(), reference, parent)
    }

    pub fn virtualized_for(&self, parent: &Locator) -> Descriptor {
        let serialized = parent.to_file_string();

        let range = Range::Virtual(VirtualRange {
            inner: Box::new(self.range.clone()),
            hash: Sha256::from_string(&serialized),
        });

        Descriptor {
            ident: self.ident.clone(),
            range,
            parent: self.parent.clone(),
        }
    }
}

pub fn descriptor_map_serializer<S>(value: &BTreeMap<Ident, Descriptor>, serializer: S) -> Result<S::Ok, S::Error> where S: serde::Serializer {
    let mut map
        = serializer.serialize_map(Some(value.len()))?;

    for v in value.values() {
        let serialized_range = v.to_file_string();

        let at_split = match serialized_range.starts_with('@') {
            true => serialized_range[1..serialized_range.len()].find('@').map(|x| x + 1),
            false => serialized_range.find('@'),
        }.unwrap();

        let ident_str = &serialized_range[0..at_split];
        let range_str = &serialized_range[at_split + 1..];

        map.serialize_entry(ident_str, range_str)?;
    }

    map.end()
}

pub fn descriptor_map_deserializer<'de, D>(deserializer: D) -> Result<BTreeMap<Ident, Descriptor>, D::Error> where D: Deserializer<'de> {
    struct MyMapVisitor {}

    impl<'de> Visitor<'de> for MyMapVisitor {
        type Value = BTreeMap<Ident, Descriptor>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a map-like structure")
        }

        fn visit_map<M>(self, mut access: M) -> Result<Self::Value, M::Error>
        where
            M: MapAccess<'de>,
        {
            let mut map
                = BTreeMap::new();

            while let Some((key, value)) = access.next_entry::<&str, &str>()? {
                let parent_marker = "::parent=";
                let parent_split = value.find(parent_marker);
            
                let ident = Ident::from_str(key)
                    .map_err(serde::de::Error::custom)?;
                let range = Range::from_file_string(&value[..parent_split.map_or(value.len(), |idx| idx)])
                    .map_err(serde::de::Error::custom)?;
        
                let parent = parent_split
                    .map(|idx| Locator::from_file_string(&value[idx + parent_marker.len()..]))
                    .transpose()
                    .map_err(serde::de::Error::custom)?;
        
                let descriptor
                    = Descriptor::new_bound(ident.clone(), range, parent);

                map.insert(ident, descriptor);
            }

            Ok(map)
        }
    }

    deserializer.deserialize_map(MyMapVisitor {})
}

impl FromFileString for Descriptor {
    type Error = Error;

    fn from_file_string(src: &str) -> Result<Self, Error> {
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
        let range = Range::from_file_string(&src[at_split + 1..parent_split.map_or(src.len(), |idx| idx)])?;

        let parent = match parent_split {
            Some(idx) => Some(Locator::from_file_string(&src[idx + parent_marker.len()..])?),
            None => None,
        };

        Ok(Descriptor::new_bound(ident, range, parent))
    }
}

impl ToFileString for Descriptor {
    fn to_file_string(&self) -> String {
        let serialized_ident = self.ident.to_file_string();
        let serialized_range = self.range.to_file_string();

        let mut final_str = String::new();
        final_str.push_str(&serialized_ident);
        final_str.push('@');
        final_str.push_str(&serialized_range);

        if let Some(parent) = &self.parent {
            final_str.push_str("::parent=");
            final_str.push_str(&parent.to_file_string());
        }

        final_str
    }
}

impl ToHumanString for Descriptor {
    fn to_print_string(&self) -> String {
        let serialized_ident = self.ident.to_print_string();
        let serialized_range = self.range.to_print_string();

        let mut final_str = String::new();
        final_str.push_str(&serialized_ident);
        final_str.push_str(&"@".truecolor(0, 175, 175).to_string());
        final_str.push_str(&serialized_range);

        final_str
    }
}

impl_serialization_traits!(Descriptor);

#[rstest]
#[case("foo@npm:1.0.0")]
#[case("foo@npm:1.0.0::parent=root@workspace:")]
fn test_descriptor_serialization(#[case] str: &str) {
    assert_eq!(str, Descriptor::from_file_string(str).unwrap().to_file_string());
}
