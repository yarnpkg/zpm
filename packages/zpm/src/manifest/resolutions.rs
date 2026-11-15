use rkyv::Archive;
use serde::{Deserialize, Deserializer};
use zpm_macro_enum::zpm_enum;
use zpm_primitives::{AnonymousSemverRange, AnonymousTagRange, Descriptor, Ident, Locator, Range, RegistrySemverRange, RegistryTagRange};
use zpm_utils::{FromFileString, ToFileString};

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
}

impl ResolutionSelector {
    /// Parses a key of the `resolutions` field.
    pub fn from_resolution_key(key: &str) -> Result<ResolutionSelector, Error> {
        let effective_key
            = key.strip_prefix("**/").unwrap_or(key);

        parse_selector(key, effective_key)
            .map_err(Error::InvalidResolution)
    }

    /// Returns the canonical selector matching the given dependency, for
    /// commands that need to add entries to the `resolutions` field.
    pub fn from_dependency(descriptor: &Descriptor) -> Result<ResolutionSelector, Error> {
        let key
            = format!("{}@{}", descriptor.ident.to_file_string(), strip_npm_protocol(&descriptor.range).to_file_string());

        Self::from_resolution_key(&key)
    }

    pub fn target_ident(&self) -> &Ident {
        match self {
            ResolutionSelector::Descriptor(params) => &params.descriptor.ident,
            ResolutionSelector::Ident(params) => &params.ident,
            ResolutionSelector::DescriptorIdent(params) => &params.ident,
            ResolutionSelector::IdentIdent(params) => &params.ident,
        }
    }

    pub fn matches(&self, parent: &Locator, parent_version: &zpm_semver::Version, descriptor: &Descriptor) -> bool {
        match self {
            ResolutionSelector::Descriptor(params) => {
                if params.descriptor.ident != descriptor.ident {
                    return false;
                }

                if params.descriptor.range != strip_npm_protocol(&descriptor.range) {
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

                if params.parent_descriptor.ident != parent.ident {
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

// Yarn 4 wrote selectors with an explicit `npm:` protocol (`yarn
// patch-commit -s` added it to every key it generated), but in a selector
// it's redundant: `pkg@npm:1.0.0` means the same thing as `pkg@1.0.0`. We
// accept it and drop it so that both spellings end up being the same
// selector, and we apply the same treatment to the dependencies we compare
// selectors against (whose ranges may carry the prefix as well).
fn strip_npm_protocol(range: &Range) -> Range {
    match range {
        Range::RegistrySemver(RegistrySemverRange {ident: None, range})
            => Range::AnonymousSemver(AnonymousSemverRange {range: range.clone()}),

        Range::RegistryTag(RegistryTagRange {ident: None, tag})
            => Range::AnonymousTag(AnonymousTagRange {tag: tag.clone()}),

        _ => range.clone(),
    }
}

// Unknown ranges end up as anonymous tags, so we need to check that what
// we got is a tag that the registry could actually serve.
fn is_registry_tag(tag: &str) -> bool {
    matches!(Range::from_file_string(&format!("npm:{tag}")), Ok(Range::RegistryTag(RegistryTagRange {ident: None, ..})))
}

// Selectors targeting a dependency are compared against its range as
// written (minus the `npm:` prefix), so they can only use ranges that
// identify a dependency without ambiguity.
fn is_valid_target_range(range: &Range) -> bool {
    match range {
        Range::AnonymousSemver(_)
            => true,

        Range::AnonymousTag(params)
            => is_registry_tag(params.tag.as_str()),

        Range::RegistrySemver(RegistrySemverRange {ident: Some(_), ..}) | Range::RegistryTag(RegistryTagRange {ident: Some(_), ..})
            => true,

        _ => false,
    }
}

fn is_path_range(range: &Range) -> bool {
    matches!(range, Range::Link(_) | Range::Portal(_) | Range::Exec(_) | Range::Tarball(_) | Range::Folder(_) | Range::Patch(_) | Range::Url(_))
}

fn invalid_selector(key: &str, reason: String) -> String {
    format!("invalid resolution selector '{key}': {reason}")
}

// Parse structurally rather than through the Range parser, which
// would misclassify ambiguous values like `1.0.0/no-deps` as Git etc.
//
// The `key` parameter is the key as written in the manifest, only used to
// build the error messages.
fn parse_selector(key: &str, effective_key: &str) -> Result<ResolutionSelector, String> {
    // Skip the `@scope/` slash when locating the parent/child split.
    let slash_search_start = if effective_key.starts_with('@') {
        effective_key.find('/').map_or(0, |idx| idx + 1)
    } else {
        0
    };

    let slash_pos = effective_key[slash_search_start..].find('/').map(|idx| idx + slash_search_start);

    let (parent_part, child_part) = match slash_pos {
        Some(idx) => (&effective_key[..idx], Some(&effective_key[idx + 1..])),
        None => (effective_key, None),
    };

    let make_anonymous = |descriptor: Descriptor| {
        let mut descriptor = descriptor;
        descriptor.range = strip_npm_protocol(&descriptor.range);
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

    // Ranges can contain slashes (scoped aliases, but also patch paths,
    // urls, file paths), in which case the split above cuts them in the
    // middle. We only have to care about it when the parent part doesn't
    // look like a valid parent, as the key would be invalid otherwise.
    if has_range_separator && child_part.is_some() {
        let parent_range = Descriptor::from_file_string(parent_part).ok()
            .map(|descriptor| make_anonymous(descriptor).range);

        if !matches!(parent_range, Some(Range::AnonymousSemver(_))) {
            if let Ok(descriptor) = Descriptor::from_file_string(effective_key).map(make_anonymous) {
                if is_valid_target_range(&descriptor.range) {
                    return Ok(ResolutionSelector::Descriptor(DescriptorResolutionSelector { descriptor }));
                }

                if is_path_range(&descriptor.range) {
                    return Err(unsupported_target_range(key, &descriptor));
                }
            }
        }
    }

    // Globs can only appear in the package names; a `*` in a range is a
    // regular semver range (`pkg@*`).
    let parent_name = if has_range_separator {
        Descriptor::from_file_string(parent_part).map_or(parent_part.to_string(), |descriptor| descriptor.ident.to_file_string())
    } else {
        parent_part.to_string()
    };

    if parent_name.contains('*') || child_part.is_some_and(|child| child.contains('*')) {
        return Err(invalid_selector(key, "glob patterns aren't supported (only a leading '**/' is tolerated); reference the packages by name instead, e.g. 'parent/child' or 'child'".to_string()));
    }

    let child_ident = child_part
        .map(|child| Ident::from_file_string(child).map_err(|_| {
            invalid_selector(key, format!("'{child}' isn't a valid package name; selectors are either 'pkg', 'pkg@range', 'parent/pkg', or 'parent@range/pkg' (a single parent level, no nested 'a/b/c' chains)"))
        }))
        .transpose()?;

    if has_range_separator {
        let descriptor = Descriptor::from_file_string(parent_part)
            .map_err(|_| invalid_selector(key, format!("'{parent_part}' isn't a valid descriptor")))?;
        let descriptor = make_anonymous(descriptor);

        match child_ident {
            Some(ident) => {
                if !matches!(descriptor.range, Range::AnonymousSemver(_)) {
                    return Err(invalid_selector(key, format!("the parent range must be a semver range (got '{}'); write it as '{}@<semver range>/{}', or '{}/{}' to match every version of the parent", descriptor.range.to_file_string(), descriptor.ident.to_file_string(), ident.to_file_string(), descriptor.ident.to_file_string(), ident.to_file_string())));
                }

                Ok(ResolutionSelector::DescriptorIdent(DescriptorIdentResolutionSelector { parent_descriptor: descriptor, ident }))
            },

            None => {
                if !is_valid_target_range(&descriptor.range) {
                    return Err(unsupported_target_range(key, &descriptor));
                }

                Ok(ResolutionSelector::Descriptor(DescriptorResolutionSelector { descriptor }))
            },
        }
    } else {
        let ident = Ident::from_file_string(parent_part)
            .map_err(|_| invalid_selector(key, format!("'{parent_part}' isn't a valid package name")))?;

        match child_ident {
            Some(child_ident) => Ok(ResolutionSelector::IdentIdent(IdentIdentResolutionSelector { parent_ident: ident, ident: child_ident })),
            None => Ok(ResolutionSelector::Ident(IdentResolutionSelector { ident })),
        }
    }
}

fn unsupported_target_range(key: &str, descriptor: &Descriptor) -> String {
    let ident
        = descriptor.ident.to_file_string();

    if let Range::Patch(params) = &descriptor.range {
        return invalid_selector(key, format!("selectors can't reference 'patch:' ranges; use the range the dependency is declared with instead, e.g. '{}@{}'", ident, strip_npm_protocol(&params.inner.0.range).to_file_string()));
    }

    invalid_selector(key, format!("selectors only support semver ranges, dist-tags and 'npm:' aliases (got '{}'); write '{}' to override every version of the package", descriptor.range.to_file_string(), ident))
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

            // Report invalid keys before reading the value, so that the error
            // location points at the key rather than at the value.
            let selector = parse_selector(&key, &effective_key)
                .map_err(de::Error::custom)?;

            let value_str: Option<String> = map.next_value()?;
            let range = value_str.as_deref().filter(|value| *value != "-").map(Range::from_file_string).transpose()
                .map_err(|_| de::Error::custom("invalid range"))?;

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

    fn parse(key: &str) -> Result<ResolutionSelector, String> {
        let mut resolutions = serde_json::Map::new();
        resolutions.insert(key.to_string(), serde_json::Value::String("1.0.0".to_string()));

        serde_json::from_value::<ResolutionsField>(serde_json::Value::Object(resolutions))
            .map(|field| field.entries[0].0.clone())
            .map_err(|error| error.to_string())
    }

    #[test]
    fn it_should_parse_supported_selectors() {
        let cases: &[(&str, &str)] = &[
            ("no-deps", "no-deps"),
            ("@scope/no-deps", "@scope/no-deps"),
            ("**/no-deps", "no-deps"),
            ("no-deps@1.0.0", "no-deps@1.0.0"),
            ("no-deps@^1.0.0", "no-deps@^1.0.0"),
            ("no-deps@npm:1.0.0", "no-deps@1.0.0"),
            ("no-deps@npm:^1.0.0", "no-deps@^1.0.0"),
            ("@scope/no-deps@1.0.0", "@scope/no-deps@1.0.0"),
            ("@scope/no-deps@npm:1.0.0", "@scope/no-deps@1.0.0"),
            ("no-deps@*", "no-deps@*"),
            ("no-deps@npm:*", "no-deps@*"),
            ("no-deps@1.x || *", "no-deps@1.x || *"),
            ("parent@*/no-deps", "parent@*/no-deps"),
            ("parent@npm:*/no-deps", "parent@*/no-deps"),
            ("alias@npm:@scope/no-deps@*", "alias@npm:@scope/no-deps@*"),
            ("no-deps@latest", "no-deps@latest"),
            ("no-deps@npm:latest", "no-deps@latest"),
            ("alias@npm:no-deps@1.0.0", "alias@npm:no-deps@1.0.0"),
            ("alias@npm:no-deps@latest", "alias@npm:no-deps@latest"),
            ("alias@npm:@scope/no-deps@1.0.0", "alias@npm:@scope/no-deps@1.0.0"),
            ("parent/no-deps", "parent/no-deps"),
            ("@scope/parent/@scope/no-deps", "@scope/parent/@scope/no-deps"),
            ("**/parent/no-deps", "parent/no-deps"),
            ("parent@1.0.0/no-deps", "parent@1.0.0/no-deps"),
            ("parent@^1.0.0/no-deps", "parent@^1.0.0/no-deps"),
            ("parent@npm:1.0.0/no-deps", "parent@1.0.0/no-deps"),
            ("parent@npm:^1.0.0/no-deps", "parent@^1.0.0/no-deps"),
            ("@scope/parent@npm:1.0.0/@scope/no-deps", "@scope/parent@1.0.0/@scope/no-deps"),
            ("express@npm:4.21.0/content-type", "express@4.21.0/content-type"),
        ];

        for &(key, canonical) in cases {
            let selector
                = parse(key).unwrap_or_else(|error| panic!("{key}: {error}"));

            assert_eq!(selector.to_file_string(), canonical, "{key}");
            assert_eq!(parse(canonical).unwrap(), selector);
        }
    }

    #[test]
    fn it_should_explain_unsupported_selectors() {
        let cases: &[(&str, &str)] = &[
            ("no-deps@workspace:^1.0.0", "invalid resolution selector 'no-deps@workspace:^1.0.0': selectors only support semver ranges, dist-tags and 'npm:' aliases (got 'workspace:^1.0.0'); write 'no-deps' to override every version of the package"),
            ("no-deps@file:./no-deps.tgz", "invalid resolution selector 'no-deps@file:./no-deps.tgz': selectors only support semver ranges, dist-tags and 'npm:' aliases"),
            ("no-deps@https://example.com/no-deps.tgz", "invalid resolution selector 'no-deps@https://example.com/no-deps.tgz': selectors only support semver ranges, dist-tags and 'npm:' aliases"),
            ("no-deps@portal:../no-deps", "invalid resolution selector 'no-deps@portal:../no-deps': selectors only support semver ranges, dist-tags and 'npm:' aliases (got 'portal:../no-deps')"),
            ("no-deps@./no-deps", "invalid resolution selector 'no-deps@./no-deps': selectors only support semver ranges, dist-tags and 'npm:' aliases"),
            ("no-deps@not a range", "invalid resolution selector 'no-deps@not a range': selectors only support semver ranges, dist-tags and 'npm:' aliases (got 'not a range')"),
            ("no-deps@patch:no-deps@npm%3A1.0.0#~/.yarn/patches/no-deps.patch", "invalid resolution selector 'no-deps@patch:no-deps@npm%3A1.0.0#~/.yarn/patches/no-deps.patch': selectors can't reference 'patch:' ranges; use the range the dependency is declared with instead, e.g. 'no-deps@1.0.0'"),
            ("no-deps@patch:no-deps@npm%3A1.0.0#no-deps.patch", "invalid resolution selector 'no-deps@patch:no-deps@npm%3A1.0.0#no-deps.patch': selectors can't reference 'patch:' ranges; use the range the dependency is declared with instead, e.g. 'no-deps@1.0.0'"),
            ("parent@latest/no-deps", "invalid resolution selector 'parent@latest/no-deps': the parent range must be a semver range (got 'latest'); write it as 'parent@<semver range>/no-deps', or 'parent/no-deps' to match every version of the parent"),
            ("parent@npm:other@1.0.0/no-deps", "invalid resolution selector 'parent@npm:other@1.0.0/no-deps': the parent range must be a semver range (got 'npm:other@1.0.0')"),
            ("parent@workspace:^1.0.0/no-deps", "invalid resolution selector 'parent@workspace:^1.0.0/no-deps': the parent range must be a semver range (got 'workspace:^1.0.0')"),
            ("a/parent/no-deps", "invalid resolution selector 'a/parent/no-deps': 'parent/no-deps' isn't a valid package name; selectors are either 'pkg', 'pkg@range', 'parent/pkg', or 'parent@range/pkg' (a single parent level, no nested 'a/b/c' chains)"),
            ("parent@1.0.0/a/no-deps", "invalid resolution selector 'parent@1.0.0/a/no-deps': 'a/no-deps' isn't a valid package name"),
            ("*/no-deps", "invalid resolution selector '*/no-deps': glob patterns aren't supported (only a leading '**/' is tolerated); reference the packages by name instead, e.g. 'parent/child' or 'child'"),
            ("parent/**/no-deps", "invalid resolution selector 'parent/**/no-deps': glob patterns aren't supported"),
            ("*@1.0.0", "invalid resolution selector '*@1.0.0': glob patterns aren't supported"),
            ("*@1.0.0/no-deps", "invalid resolution selector '*@1.0.0/no-deps': glob patterns aren't supported"),
            ("**/parent/**/no-deps", "invalid resolution selector '**/parent/**/no-deps': glob patterns aren't supported"),
            ("@scope", "invalid resolution selector '@scope': '@scope' isn't a valid package name"),
        ];

        for &(key, message) in cases {
            let error
                = parse(key).unwrap_err();

            assert!(error.starts_with(message), "unexpected error for {key}: {error}");
        }
    }

    fn dependency(descriptor: &str) -> Descriptor {
        Descriptor::from_file_string(descriptor).unwrap()
    }

    #[test]
    fn it_should_match_descriptor_selectors_regardless_of_the_npm_prefix() {
        let cases: &[(&str, &str, bool)] = &[
            ("no-deps@1.0.0", "no-deps@1.0.0", true),
            ("no-deps@1.0.0", "no-deps@npm:1.0.0", true),
            ("no-deps@npm:1.0.0", "no-deps@1.0.0", true),
            ("no-deps@npm:1.0.0", "no-deps@npm:1.0.0", true),
            ("no-deps@*", "no-deps@*", true),
            ("no-deps@npm:*", "no-deps@*", true),
            ("no-deps@*", "no-deps@npm:*", true),
            ("no-deps@*", "no-deps@1.0.0", false),
            ("no-deps@1.0.0", "no-deps@^1.0.0", false),
            ("no-deps@^1.0.0", "no-deps@1.0.0", false),
            ("no-deps@1.0.0", "no-deps@npm:other@1.0.0", false),
            ("no-deps@latest", "no-deps@npm:latest", true),
            ("no-deps@npm:latest", "no-deps@latest", true),
            ("alias@npm:no-deps@1.0.0", "alias@npm:no-deps@1.0.0", true),
            ("alias@npm:no-deps@1.0.0", "alias@1.0.0", false),
            ("@scope/no-deps@npm:1.0.0", "@scope/no-deps@1.0.0", true),
            ("no-deps@1.0.0", "other@1.0.0", false),
        ];

        for &(key, descriptor, expected) in cases {
            let selector
                = parse(key).unwrap_or_else(|error| panic!("{key}: {error}"));

            let parent
                = Locator::from_file_string("parent@npm:1.0.0").unwrap();
            let parent_version
                = zpm_semver::Version::from_file_string("1.0.0").unwrap();

            assert_eq!(selector.matches(&parent, &parent_version, &dependency(descriptor)), expected, "{key} vs {descriptor}");
        }
    }

    #[test]
    fn it_should_match_parent_selectors_written_with_the_npm_prefix() {
        let cases: &[(&str, &str, bool)] = &[
            ("parent@npm:1.0.0/no-deps", "1.0.0", true),
            ("parent@npm:^1.0.0/no-deps", "1.2.0", true),
            ("parent@npm:1.0.0/no-deps", "1.2.0", false),
        ];

        for &(key, version, expected) in cases {
            let selector
                = parse(key).unwrap_or_else(|error| panic!("{key}: {error}"));

            let parent
                = Locator::from_file_string(&format!("parent@npm:{version}")).unwrap();
            let parent_version
                = zpm_semver::Version::from_file_string(version).unwrap();

            assert_eq!(selector.matches(&parent, &parent_version, &dependency("no-deps@^1.0.0")), expected, "{key} vs {version}");
        }
    }

    #[test]
    fn it_should_build_canonical_selectors_from_dependencies() {
        assert_eq!(ResolutionSelector::from_dependency(&dependency("has-flag@npm:^4.0.0")).unwrap().to_file_string(), "has-flag@^4.0.0");
        assert_eq!(ResolutionSelector::from_dependency(&dependency("@scope/has-flag@npm:4.0.0")).unwrap().to_file_string(), "@scope/has-flag@4.0.0");
        assert_eq!(ResolutionSelector::from_dependency(&dependency("has-flag@npm:*")).unwrap().to_file_string(), "has-flag@*");
        assert_eq!(ResolutionSelector::from_dependency(&dependency("has-flag@npm:latest")).unwrap().to_file_string(), "has-flag@latest");
        assert_eq!(ResolutionSelector::from_dependency(&dependency("alias@npm:has-flag@4.0.0")).unwrap().to_file_string(), "alias@npm:has-flag@4.0.0");
        assert!(ResolutionSelector::from_dependency(&dependency("has-flag@https://example.com/has-flag.tgz")).is_err());
    }

    #[test]
    fn removal_resolutions_serialize_as_dashes() {
        let resolutions: ResolutionsField
            = serde_json::from_str(r#"{"parent/child":null,"parent/peer":"-","@types/react-native":null}"#).unwrap();

        assert_eq!(
            serde_json::to_string(&resolutions).unwrap(),
            r#"{"parent/child":"-","parent/peer":"-","@types/react-native":"-"}"#,
        );
    }
}
