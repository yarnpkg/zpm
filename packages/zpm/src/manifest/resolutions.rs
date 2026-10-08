use rkyv::Archive;
use serde::{Deserialize, Deserializer};
use zpm_macro_enum::zpm_enum;
use zpm_primitives::{Descriptor, Ident, Locator, Range, RegistrySemverRange};
use zpm_utils::{FromFileString, ToFileString};
use zpm_semver::pubgrub::ToRanges;

use crate::{
    error::Error,
};

#[zpm_enum(or_else = |s| Err(Error::InvalidResolution(s.to_string())))]
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Archive, rkyv::Serialize, rkyv::Deserialize)]
#[derive_variants(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Archive, rkyv::Serialize, rkyv::Deserialize)]
#[variant_struct_attr(rkyv(derive(PartialEq, Eq, PartialOrd, Ord, Hash)))]
pub enum ResolutionSelector {
    #[pattern(r"^(?<descriptor>.*)$")]
    #[to_file_string(|params| params.descriptor.to_file_string())]
    #[to_print_string(|params| params.descriptor.to_print_string())]
    Descriptor {
        descriptor: Descriptor,
    },

    #[pattern(r"^(?<ident>.*)$")]
    #[to_file_string(|params| params.ident.to_file_string())]
    #[to_print_string(|params| params.ident.to_print_string())]
    Ident {
        ident: Ident,
    },

    #[pattern(r"^(?<parent_descriptor>(?:@[^/*]*/)?[^/*]+)/(?<ident>[^*]+)$")]
    #[to_file_string(|params| format!("{}/{}", params.parent_descriptor.to_file_string(), params.ident.to_file_string()))]
    #[to_print_string(|params| format!("{}/{}", params.parent_descriptor.to_print_string(), params.ident.to_print_string()))]
    DescriptorIdent {
        parent_descriptor: Descriptor,
        ident: Ident,
    },

    #[pattern(r"^(?<parent_ident>(?:@[^/*]*/)?[^/*]+)/(?<ident>[^*]+)$")]
    #[to_file_string(|params| format!("{}/{}", params.parent_ident.to_file_string(), params.ident.to_file_string()))]
    #[to_print_string(|params| format!("{}/{}", params.parent_ident.to_print_string(), params.ident.to_print_string()))]
    IdentIdent {
        parent_ident: Ident,
        ident: Ident,
    },

    #[pattern(r"^(?<ident>.*)@intersects:(?<range>.*)$")]
    #[to_file_string(|params| format!("{}@intersects:{}", params.ident.to_file_string(), params.range.to_file_string()))]
    #[to_print_string(|params| format!("{}@intersects:{}", params.ident.to_print_string(), params.range.to_print_string()))]
    Intersecting {
        ident: Ident,
        range: zpm_semver::Range,
    },
}

impl ResolutionSelector {
    pub fn target_ident(&self) -> &Ident {
        match self {
            ResolutionSelector::Descriptor(params) => &params.descriptor.ident,
            ResolutionSelector::Ident(params) => &params.ident,
            ResolutionSelector::DescriptorIdent(params) => &params.ident,
            ResolutionSelector::IdentIdent(params) => &params.ident,
            ResolutionSelector::Intersecting(params) => &params.ident,
        }
    }

    pub fn matches(&self, parent: &Locator, parent_version: &zpm_semver::Version, descriptor: &Descriptor) -> bool {
        match self {
            ResolutionSelector::Intersecting(params) => {
                if params.ident != descriptor.ident {
                    return false;
                }
                let range = match &descriptor.range {
                    Range::AnonymousSemver(params) => &params.range,
                    Range::RegistrySemver(params) if params.ident.is_none() => &params.range,
                    _ => return false,
                };
                !params.range.to_ranges().intersection(&range.to_ranges()).is_empty()
            },
            ResolutionSelector::Descriptor(params) => {
                if params.descriptor != *descriptor {
                    return false;
                }

                true
            },

            ResolutionSelector::Ident(params) => {
                if params.ident != descriptor.ident {
                    return false;
                }

                true
            },

            ResolutionSelector::DescriptorIdent(params) => {
                if params.ident != descriptor.ident {
                    return false;
                }

                if let Range::AnonymousSemver(parent_params) = &params.parent_descriptor.range {
                    if !parent_params.range.check(parent_version) {
                        return false;
                    }
                } else {
                    return false;
                }

                true
            },

            ResolutionSelector::IdentIdent(params) => {
                if params.ident != descriptor.ident {
                    return false;
                }

                if params.parent_ident != parent.ident {
                    return false;
                }

                true
            },
        }
    }

    pub fn apply(&self, parent: &Locator, parent_version: &zpm_semver::Version, descriptor: &Descriptor, replacement_range: &Range) -> Option<Range> {
        self.matches(parent, parent_version, descriptor).then(|| replacement_range.clone())
    }

}



use serde::{ser::SerializeMap, Serialize, Serializer};
use serde::de::{self, Visitor, MapAccess};
use std::fmt;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, PartialEq, Eq, Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct ResolutionsField {
    pub entries: Vec<(ResolutionSelector, Option<Range>)>,
    pub by_ident: BTreeMap<Ident, Vec<(ResolutionSelector, Option<Range>)>>,
    pub legacy_glob_keys: Vec<String>,
}

impl ResolutionsField {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            by_ident: BTreeMap::new(),
            legacy_glob_keys: Vec::new(),
        }
    }

    pub fn from_entries(entries: impl IntoIterator<Item = (ResolutionSelector, Option<Range>)>) -> Self {
        let mut field
            = Self::new();

        for (selector, range) in entries {
            field.add_entry(selector, range);
        }

        field
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&ResolutionSelector, &Option<Range>)> {
        self.entries.iter().map(|(k, v)| (k, v))
    }

    pub fn get_by_ident(&self, ident: &Ident) -> Option<&Vec<(ResolutionSelector, Option<Range>)>> {
        self.by_ident.get(ident)
    }

    fn add_entry(&mut self, selector: ResolutionSelector, range: Option<Range>) {
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
            map.serialize_entry(&key.to_file_string(), &value.as_ref().map(ToFileString::to_file_string).unwrap_or_else(|| "-".to_string()))?;
        }
        map.end()
    }
}

// Parse structurally rather than through the Range parser, which
// would misclassify ambiguous values like `1.0.0/no-deps` as Git etc.
fn parse_selector(key: &str) -> Option<ResolutionSelector> {
    use zpm_primitives::AnonymousSemverRange;

    if let Some((ident, range)) = key.split_once("@intersects:") {
        return Some(ResolutionSelector::Intersecting(IntersectingResolutionSelector {
            ident: Ident::from_file_string(ident).ok()?,
            range: zpm_semver::Range::from_file_string(range).ok()?,
        }));
    }

    // Skip the `@scope/` slash when locating the parent/child split.
    let slash_search_start = if key.starts_with('@') {
        key.find('/').map_or(0, |idx| idx + 1)
    } else {
        0
    };

    let slash_pos = key[slash_search_start..].find('/').map(|idx| idx + slash_search_start);

    let (parent_part, child_part) = match slash_pos {
        Some(idx) => (&key[..idx], Some(&key[idx + 1..])),
        None => (key, None),
    };

    let make_anonymous = |descriptor: Descriptor| {
        let mut descriptor = descriptor;
        let raw_range = descriptor.range.to_file_string();
        if let Ok(range) = zpm_semver::Range::from_file_string(&raw_range) {
            descriptor.range = Range::AnonymousSemver(AnonymousSemverRange { range });
        }
        descriptor
    };

    // True when `parent_part` has an explicit `@<range>`. Strip a
    // leading `@scope` first so a scope's `@` doesn't masquerade as
    // the descriptor separator.
    let has_range_separator = if let Some(rest) = parent_part.strip_prefix('@') {
        rest.contains('@')
    } else {
        parent_part.contains('@')
    };

    let parent_descriptor = if has_range_separator {
        let descriptor = Descriptor::from_file_string(parent_part).ok()?;
        Some(make_anonymous(descriptor))
    } else {
        None
    };

    let parent_ident = if parent_descriptor.is_none() {
        Some(zpm_primitives::Ident::from_file_string(parent_part).ok()?)
    } else {
        None
    };

    match (parent_descriptor, parent_ident, child_part) {
        (Some(descriptor), _, None) => Some(ResolutionSelector::Descriptor(DescriptorResolutionSelector { descriptor })),
        (_, Some(ident), None) => Some(ResolutionSelector::Ident(IdentResolutionSelector { ident })),
        (Some(parent_descriptor), _, Some(child)) => {
            let ident = zpm_primitives::Ident::from_file_string(child).ok()?;
            Some(ResolutionSelector::DescriptorIdent(DescriptorIdentResolutionSelector { parent_descriptor, ident }))
        },
        (_, Some(parent_ident), Some(child)) => {
            let ident = zpm_primitives::Ident::from_file_string(child).ok()?;
            Some(ResolutionSelector::IdentIdent(IdentIdentResolutionSelector { parent_ident, ident }))
        },
        _ => None,
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
            let (effective_key, legacy_form) = if let Some(stripped) = key.strip_prefix("**/") {
                (stripped.to_string(), Some(key.clone()))
            } else {
                (key.clone(), None)
            };

            let selector = parse_selector(&effective_key)
                .ok_or_else(|| de::Error::custom("invalid resolution selector"))?;

            let value_str: Option<String> = map.next_value()?;
            let range = value_str.as_deref().filter(|value| *value != "-").map(Range::from_file_string).transpose()
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
                | ResolutionSelector::Intersecting(_)
            );

            if !is_valid_resolution_descriptor {
                return Err(de::Error::custom("the range must be an anonymous semver range"));
            }

            if let Some(legacy) = legacy_form {
                field.legacy_glob_keys.push(legacy);
            }

            field.add_entry(selector, range);
        }

        Ok(field)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intersection_selectors_match_overlapping_ranges_without_changing_exact_selectors() {
        let intersection = parse_selector("child@intersects:1.0.0").unwrap();
        let exact = parse_selector("child@1.0.0").unwrap();
        let parent = Locator::from_file_string("parent@npm:1.0.0").unwrap();
        let version = zpm_semver::Version::from_file_string("1.0.0").unwrap();
        for (range, intersects, equals) in [("^1.0.0", true, false), ("1.0.0", true, true), ("^2.0.0", false, false), ("npm:other@^1.0.0", false, false)] {
            let descriptor = Descriptor::from_file_string(&format!("child@{range}")).unwrap();
            assert_eq!(intersection.matches(&parent, &version, &descriptor), intersects);
            assert_eq!(exact.matches(&parent, &version, &descriptor), equals);
        }
        let field: ResolutionsField = serde_json::from_str(r#"{"child@intersects:1.0.0":"2.0.0"}"#).unwrap();
        assert_eq!(serde_json::to_string(&field).unwrap(), r#"{"child@intersects:1.0.0":"2.0.0"}"#);
    }
}
