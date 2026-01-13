use bincode::{Decode, Encode};
use serde::{Deserialize, Deserializer};
use zpm_macro_enum::zpm_enum;
use zpm_primitives::{Descriptor, Ident, Locator, Range, RegistrySemverRange};
use zpm_utils::{impl_file_string_from_str, impl_file_string_serialization, FromFileString, ToFileString, ToHumanString};

use crate::{
    error::Error,
};

#[zpm_enum(or_else = |s| Err(Error::InvalidResolution(s.to_string())))]
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Encode, Decode)]
#[derive_variants(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Encode, Decode)]
pub enum ResolutionSelector {
    #[pattern(r"^(?<descriptor>.*)$")]
    Descriptor {
        descriptor: Descriptor,
    },

    #[pattern(r"^(?<ident>.*)$")]
    Ident {
        ident: Ident,
    },

    #[pattern(r"^(?<parent_descriptor>(?:@[^/*]*/)?[^/*]+)/(?<ident>[^*]+)$")]
    DescriptorIdent {
        parent_descriptor: Descriptor,
        ident: Ident,
    },

    #[pattern(r"^(?<parent_ident>(?:@[^/*]*/)?[^/*]+)/(?<ident>[^*]+)$")]
    IdentIdent {
        parent_ident: Ident,
        ident: Ident,
    },
}

impl ResolutionSelector {
    pub fn target_ident(&self) -> &Ident {
        match self {
            ResolutionSelector::Descriptor(params) => &params.descriptor.ident,
            ResolutionSelector::Ident(params) => &params.ident,
            ResolutionSelector::DescriptorIdent(params) => &params.ident,
            ResolutionSelector::IdentIdent(params) => &params.ident,
        }
    }

    pub fn apply(&self, parent: &Locator, parent_version: &zpm_semver::Version, descriptor: &Descriptor, replacement_range: &Range) -> Option<Range> {
        match self {
            ResolutionSelector::Descriptor(params) => {
                if params.descriptor != *descriptor {
                    return None;
                }

                Some(replacement_range.clone())
            },

            ResolutionSelector::Ident(params) => {
                if params.ident != descriptor.ident {
                    return None;
                }

                Some(replacement_range.clone())
            },

            ResolutionSelector::DescriptorIdent(params) => {
                if params.ident != descriptor.ident {
                    return None;
                }

                if let Range::AnonymousSemver(parent_params) = &params.parent_descriptor.range {
                    if !parent_params.range.check(parent_version) {
                        return None;
                    }
                } else {
                    return None;
                }

                Some(replacement_range.clone())
            },

            ResolutionSelector::IdentIdent(params) => {
                if params.ident != descriptor.ident {
                    return None;
                }

                if params.parent_ident != parent.ident {
                    return None;
                }

                Some(replacement_range.clone())
            },
        }
    }
}

impl ToFileString for ResolutionSelector {
    fn to_file_string(&self) -> String {
        match self {
            ResolutionSelector::Descriptor(params) => {
                params.descriptor.to_file_string()
            },

            ResolutionSelector::Ident(params) => {
                params.ident.to_file_string()
            },

            ResolutionSelector::DescriptorIdent(params) => {
                params.parent_descriptor.to_file_string() + "/" + &params.ident.to_file_string()
            },

            ResolutionSelector::IdentIdent(params) => {
                params.parent_ident.to_file_string() + "/" + &params.ident.to_file_string()
            },
        }
    }
}

impl ToHumanString for ResolutionSelector {
    fn to_print_string(&self) -> String {
        self.to_file_string()
    }
}

impl_file_string_from_str!(ResolutionSelector);
impl_file_string_serialization!(ResolutionSelector);

use serde::{ser::SerializeMap, Serialize, Serializer};
use serde::de::{self, Visitor, MapAccess};
use std::fmt;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, Encode, Decode, PartialEq, Eq)]
pub struct ResolutionsField {
    pub entries: Vec<(ResolutionSelector, Range)>,
    pub by_ident: BTreeMap<Ident, Vec<(ResolutionSelector, Range)>>,
}

impl ResolutionsField {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            by_ident: BTreeMap::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&ResolutionSelector, &Range)> {
        self.entries.iter().map(|(k, v)| (k, v))
    }

    pub fn get_by_ident(&self, ident: &Ident) -> Option<&Vec<(ResolutionSelector, Range)>> {
        self.by_ident.get(ident)
    }

    fn add_entry(&mut self, selector: ResolutionSelector, range: Range) {
        let target_ident
            = selector.target_ident();

        self.entries.push((selector.clone(), range.clone()));
        self.by_ident
            .entry(target_ident.clone())
            .or_default()
            .push((selector, range));
    }
}

impl<'de> Deserialize<'de> for ResolutionsField {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>
    {
        deserializer.deserialize_map(ResolutionsFieldVisitor)
    }
}

impl Serialize for ResolutionsField {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer
    {
        let mut map = serializer.serialize_map(Some(self.entries.len()))?;
        for (key, value) in &self.entries {
            map.serialize_entry(&key.to_file_string(), &value.to_file_string())?;
        }
        map.end()
    }
}

struct ResolutionsFieldVisitor;

impl<'de> Visitor<'de> for ResolutionsFieldVisitor {
    type Value = ResolutionsField;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a map of resolution selectors to ranges")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>
    {
        let mut field = ResolutionsField::new();

        while let Some(key) = map.next_key::<String>()? {
            let selector = ResolutionSelector::from_file_string(&key)
                .map_err(|_| de::Error::custom("invalid resolution selector"))?;

            let value_str: String = map.next_value()?;
            let range = Range::from_file_string(&value_str)
                .map_err(|_| de::Error::custom("invalid range"))?;

            // TODO: Remove this in a future major version; we're keeping it for backwards compatibility with
            // the Berry codebase in which `yarn patch` was adding the "npm:" prefix to all descriptors.
            if matches!(selector, ResolutionSelector::Descriptor(DescriptorResolutionSelector {descriptor: Descriptor {range: Range::RegistrySemver(RegistrySemverRange {ident: None, ..}), ..}, ..})) {
                return Err(de::Error::custom("the 'npm:' prefix is no longer needed"));
            }

            let is_valid_resolution_descriptor = matches!(selector,
                | ResolutionSelector::Descriptor(DescriptorResolutionSelector {descriptor: Descriptor {range: Range::AnonymousSemver(_), ..}, ..})
                | ResolutionSelector::DescriptorIdent(DescriptorIdentResolutionSelector {parent_descriptor: Descriptor {range: Range::AnonymousSemver(_), ..}, ..})
                | ResolutionSelector::Ident(_)
                | ResolutionSelector::IdentIdent(_)
            );

            if !is_valid_resolution_descriptor {
                return Err(de::Error::custom("the range must be an anonymous semver range"));
            }

            field.add_entry(selector, range);
        }

        Ok(field)
    }
}
