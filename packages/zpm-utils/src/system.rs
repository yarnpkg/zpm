use std::str::FromStr;
use std::sync::LazyLock;

use rkyv::Archive;
use serde::{Deserialize, Serialize};
use zpm_macro_enum::zpm_enum;

use crate::{EnumError, FromFileString, Path, ToFileString};

use crate as zpm_utils;

pub fn get_system_string() -> &'static str {
    env!("TARGET")
}

const LDD_PATH: &str = "/usr/bin/ldd";

#[cfg(target_arch = "x86_64")]
const ARCH: Cpu = Cpu::X86_64;

#[cfg(target_arch = "aarch64")]
const ARCH: Cpu = Cpu::Aarch64;

#[cfg(target_arch = "x86")]
const ARCH: Cpu = Cpu::I386;

#[cfg(target_arch = "wasm64")]
const ARCH: Cpu = Cpu::Wasm64;

#[cfg(target_os = "linux")]
const OS: Os = Os::Linux;

#[cfg(target_os = "macos")]
const OS: Os = Os::MacOS;

#[cfg(target_os = "windows")]
const OS: Os = Os::Windows;

#[cfg(all(target_os = "linux", target_env = "gnu"))]
const LIBC: Option<Libc> = Some(Libc::Glibc);

#[cfg(target_env = "musl")]
const LIBC: Option<Libc> = Some(Libc::Musl);

#[cfg(not(any(all(target_os = "linux", target_env = "gnu"), target_env = "musl")))]
const LIBC: Option<Libc> = None;

fn detect_libc() -> Option<Libc> {
    // The libc requirement only makes sense on Linux; on Windows `/usr/bin/ldd`
    // would resolve against the current drive and could pick up a Git Bash or
    // Cygwin installation.
    if cfg!(windows) {
        return LIBC;
    }

    let ldd_contents
        = Path::from_str(LDD_PATH).unwrap()
            .fs_read_text_prealloc()
            .ok();

    if let Some(ldd_contents) = ldd_contents {
        if ldd_contents.contains("GLIBC") || ldd_contents.contains("GNU libc") || ldd_contents.contains("GNU C Library") {
            return Some(Libc::Glibc);
        }

        if ldd_contents.contains("musl") {
            return Some(Libc::Musl);
        }
    }

    LIBC
}

#[derive(Debug)]
pub struct System {
    pub arch: Option<Cpu>,
    pub os: Option<Os>,
    pub libc: Option<Libc>,
}

impl System {
    pub const fn new(arch: Option<Cpu>, os: Option<Os>, libc: Option<Libc>) -> Self {
        Self {
            arch,
            os,
            libc,
        }
    }

    pub fn without_libc(&self) -> Self {
        Self {
            arch: self.arch.clone(),
            os: self.os.clone(),
            libc: None,
        }
    }

    pub fn to_requirements(&self) -> Requirements {
        Requirements {
            arch: self.arch.clone().into_iter().map(RequirementEntry::Include).collect(),
            os: self.os.clone().into_iter().map(RequirementEntry::Include).collect(),
            libc: self.libc.clone().into_iter().map(RequirementEntry::Include).collect(),
        }
    }
}

/// A set of systems, described by one list of supported values per field.
/// The systems covered by the set are the cross product of all the fields;
/// a `None` field means that all values are supported, whereas an empty
/// list means that none are.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SystemSet {
    pub arch: Option<Vec<Cpu>>,
    pub os: Option<Vec<Os>>,
    pub libc: Option<Vec<Libc>>,
}

impl SystemSet {
    /// The set only containing the system we're currently running on.
    pub fn from_current() -> Self {
        let current
            = System::current();

        Self {
            arch: Some(current.arch.iter().cloned().collect()),
            os: Some(current.os.iter().cloned().collect()),
            libc: Some(current.libc.iter().cloned().collect()),
        }
    }
}

impl ToFileString for System {
    fn to_file_string(&self) -> String {
        let mut segments
            = vec![];

        if let Some(os) = &self.os {
            segments.push(os.to_file_string());
        }

        if let Some(arch) = &self.arch {
            segments.push(arch.to_file_string());
        }

        if let Some(libc) = &self.libc {
            segments.push(libc.to_file_string());
        }

        if segments.is_empty() {
            return "unknown".to_string();
        }

        segments.join("-")
    }
}

static CURRENT_DESCRIPTION: LazyLock<System> = LazyLock::new(|| {
    System::from_current()
});

impl System {
    pub fn current() -> &'static Self {
        &*CURRENT_DESCRIPTION
    }

    pub fn from_current() -> Self {
        let arch = std::env::var("YARN_CPU_OVERRIDE").ok()
            .map_or(Some(ARCH), |s| Some(Cpu::from_str(&s).unwrap()));

        let os = std::env::var("YARN_OS_OVERRIDE").ok()
            .map_or(Some(OS), |s| Some(Os::from_str(&s).unwrap()));

        let libc = std::env::var("YARN_LIBC_OVERRIDE").ok()
            .map_or(detect_libc(), |s| Some(Libc::from_str(&s).unwrap()));

        Self {
            arch,
            os,
            libc,
        }
    }
}

#[zpm_enum(error = EnumError, or_else = |s| Err(EnumError::NotFound(s.to_string())))]
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Archive, rkyv::Serialize, rkyv::Deserialize)]
pub enum Cpu {
    #[literal("current")]
    Current,

    #[literal("ia32")]
    I386,

    #[literal("x64")]
    X86_64,

    #[literal("arm64")]
    Aarch64,

    #[literal("wasm64")]
    Wasm64,

    #[fallback]
    Other(String),
}

#[zpm_enum(error = EnumError, or_else = |s| Err(EnumError::NotFound(s.to_string())))]
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Archive, rkyv::Serialize, rkyv::Deserialize)]
pub enum Libc {
    #[literal("current")]
    Current,

    #[literal("glibc")]
    Glibc,

    #[literal("musl")]
    Musl,

    #[fallback]
    Other(String),
}

#[zpm_enum(error = EnumError, or_else = |s| Err(EnumError::NotFound(s.to_string())))]
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Archive, rkyv::Serialize, rkyv::Deserialize)]
pub enum Os {
    #[literal("current")]
    Current,

    #[literal("darwin")]
    MacOS,

    #[literal("linux")]
    Linux,

    #[literal("win32")]
    Windows,

    #[fallback]
    Other(String),
}

/// One entry of the `os`, `cpu`, or `libc` fields of a manifest. As in npm,
/// an entry prefixed by `!` excludes the value rather than including it
/// (`"os": ["!win32"]` means "any OS but Windows").
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Archive, rkyv::Serialize, rkyv::Deserialize)]
pub enum RequirementEntry<T> {
    Include(T),
    Exclude(T),
}

impl<T: FromFileString> FromFileString for RequirementEntry<T> {
    type Error = T::Error;

    fn from_file_string(src: &str) -> Result<Self, Self::Error> {
        // Only the first `!` is a negation marker; just like npm, `!!win32`
        // excludes the literal `!win32` value (so nothing in practice)
        match src.strip_prefix('!') {
            Some(value)
                => Ok(RequirementEntry::Exclude(T::from_file_string(value)?)),

            None
                => Ok(RequirementEntry::Include(T::from_file_string(src)?)),
        }
    }
}

impl<T: ToFileString> ToFileString for RequirementEntry<T> {
    fn to_file_string(&self) -> String {
        match self {
            RequirementEntry::Include(value)
                => value.to_file_string(),

            RequirementEntry::Exclude(value)
                => format!("!{}", value.to_file_string()),
        }
    }
}

zpm_utils::impl_file_string_serialization!(RequirementEntry<Cpu>);
zpm_utils::impl_file_string_serialization!(RequirementEntry<Os>);
zpm_utils::impl_file_string_serialization!(RequirementEntry<Libc>);

/// The set of values accepted by a list of requirement entries, in a form
/// that can be unioned exactly.
enum AcceptedValues<T> {
    /// Only these values are accepted.
    Only(Vec<T>),

    /// Every value is accepted except these ones.
    AllBut(Vec<T>),
}

impl<T: Clone + PartialEq + ToFileString> AcceptedValues<T> {
    fn from_entries(entries: &[RequirementEntry<T>]) -> Self {
        if entries.is_empty() || is_any(entries) {
            return AcceptedValues::AllBut(vec![]);
        }

        let excluded
            = excluded_values(entries);

        if !has_included_values(entries) {
            return AcceptedValues::AllBut(excluded);
        }

        let included = entries.iter()
            .filter_map(|entry| match entry {
                RequirementEntry::Include(value) if !excluded.contains(value) => Some(value.clone()),
                _ => None,
            })
            .collect();

        AcceptedValues::Only(included)
    }

    fn union(self, other: Self) -> Self {
        match (self, other) {
            (AcceptedValues::Only(mut values), AcceptedValues::Only(other)) => {
                for value in other {
                    if !values.contains(&value) {
                        values.push(value);
                    }
                }

                AcceptedValues::Only(values)
            },

            (AcceptedValues::AllBut(excluded), AcceptedValues::Only(included)) | (AcceptedValues::Only(included), AcceptedValues::AllBut(excluded)) => {
                AcceptedValues::AllBut(excluded.into_iter().filter(|value| !included.contains(value)).collect())
            },

            (AcceptedValues::AllBut(excluded), AcceptedValues::AllBut(other)) => {
                AcceptedValues::AllBut(excluded.into_iter().filter(|value| other.contains(value)).collect())
            },
        }
    }
}

/// npm gives a special meaning to lists only containing `any`.
fn is_any<T: ToFileString>(entries: &[RequirementEntry<T>]) -> bool {
    matches!(entries, [RequirementEntry::Include(value)] if value.to_file_string() == "any")
}

fn has_included_values<T>(entries: &[RequirementEntry<T>]) -> bool {
    entries.iter().any(|entry| matches!(entry, RequirementEntry::Include(_)))
}

fn excluded_values<T: Clone>(entries: &[RequirementEntry<T>]) -> Vec<T> {
    entries.iter()
        .filter_map(|entry| match entry {
            RequirementEntry::Exclude(value) => Some(value.clone()),
            _ => None,
        })
        .collect()
}

/// Whether a value is accepted by a non-empty list of entries. This follows
/// npm's `checkList`: the value must match none of the excluded entries, and
/// one of the included entries if there's any.
fn is_value_accepted<T: PartialEq + ToFileString>(entries: &[RequirementEntry<T>], value: &T) -> bool {
    if is_any(entries) {
        return true;
    }

    let mut has_included_values
        = false;
    let mut is_included
        = false;

    for entry in entries {
        match entry {
            RequirementEntry::Exclude(excluded) => {
                if excluded == value {
                    return false;
                }
            },

            RequirementEntry::Include(included) => {
                has_included_values = true;
                is_included = is_included || included == value;
            },
        }
    }

    is_included || !has_included_values
}

/// Whether a field of a system satisfies the given entries. Like npm, we
/// consider that a system whose value is unknown (typically the libc on
/// systems other than Linux) doesn't satisfy any list, even negated ones.
fn is_field_valid<T: PartialEq + ToFileString>(entries: &[RequirementEntry<T>], value: Option<&T>) -> bool {
    if entries.is_empty() {
        return true;
    }

    value.map_or(false, |value| is_value_accepted(entries, value))
}

/// Whether at least one of the supported values satisfies the given entries
/// (a `None` list meaning that all values are supported).
fn is_field_valid_for_set<T: Clone + PartialEq + ToFileString>(entries: &[RequirementEntry<T>], supported: &Option<Vec<T>>) -> bool {
    if entries.is_empty() {
        return true;
    }

    match supported {
        Some(supported)
            => supported.iter().any(|value| is_value_accepted(entries, value)),

        None => match AcceptedValues::from_entries(entries) {
            AcceptedValues::Only(values) => !values.is_empty(),
            AcceptedValues::AllBut(_) => true,
        },
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq, Hash, PartialOrd, Ord, Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct Requirements {
    #[serde(default)]
    #[serde(rename = "cpu")]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[serde(deserialize_with = "deserialize_requirement_list")]
    arch: Vec<RequirementEntry<Cpu>>,

    #[serde(default)]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[serde(deserialize_with = "deserialize_requirement_list")]
    os: Vec<RequirementEntry<Os>>,

    #[serde(default)]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[serde(deserialize_with = "deserialize_requirement_list")]
    libc: Vec<RequirementEntry<Libc>>,
}

/// The `os`, `cpu`, and `libc` fields are meant to be arrays, but npm also
/// accepts a single string (its documentation even uses that form for
/// `libc`), so some packages are published with `"libc": "musl"`. We treat
/// such strings as one-element lists, and `null` as an absent field.
fn deserialize_requirement_list<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    struct RequirementListVisitor<T>(std::marker::PhantomData<T>);

    impl<'de, T: Deserialize<'de>> serde::de::Visitor<'de> for RequirementListVisitor<T> {
        type Value = Vec<T>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a string or a sequence of strings")
        }

        fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Self::Value, E> {
            let value
                = T::deserialize(serde::de::value::StrDeserializer::<E>::new(value))?;

            Ok(vec![value])
        }

        fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
            Ok(vec![])
        }

        fn visit_none<E: serde::de::Error>(self) -> Result<Self::Value, E> {
            Ok(vec![])
        }

        fn visit_some<D2: serde::Deserializer<'de>>(self, deserializer: D2) -> Result<Self::Value, D2::Error> {
            deserialize_requirement_list(deserializer)
        }

        fn visit_seq<A: serde::de::SeqAccess<'de>>(self, seq: A) -> Result<Self::Value, A::Error> {
            Vec::<T>::deserialize(serde::de::value::SeqAccessDeserializer::new(seq))
        }
    }

    deserializer.deserialize_any(RequirementListVisitor(std::marker::PhantomData))
}

impl Requirements {
    /// Widens the requirements to also accept the systems the other
    /// requirements accept. Only exact for requirements differing on a
    /// single field (which is what a multi-platform artifact produces, like
    /// a macOS universal2 wheel covering both arm64 and x64).
    pub fn extend(&mut self, other: &Requirements) {
        fn union<T: Clone + PartialEq + ToFileString>(target: &mut Vec<RequirementEntry<T>>, source: &[RequirementEntry<T>]) {
            // An empty list accepts everything; it can't be widened
            if target.is_empty() {
                return;
            }

            if source.is_empty() {
                target.clear();
                return;
            }

            let accepted
                = AcceptedValues::from_entries(target)
                    .union(AcceptedValues::from_entries(source));

            // Note that an empty list also accepts the systems for which the
            // field is unknown, which no other list does; we accept being a
            // little too permissive in that case
            *target = match accepted {
                // Neither list accepted anything; neither does the union
                AcceptedValues::Only(values) if values.is_empty() => {
                    return;
                },

                AcceptedValues::Only(values)
                    => values.into_iter().map(RequirementEntry::Include).collect(),

                AcceptedValues::AllBut(values)
                    => values.into_iter().map(RequirementEntry::Exclude).collect(),
            };
        }

        union(&mut self.arch, &other.arch);
        union(&mut self.os, &other.os);
        union(&mut self.libc, &other.libc);
    }

    pub fn is_conditional(&self) -> bool {
        !self.arch.is_empty() || !self.os.is_empty() || !self.libc.is_empty()
    }

    pub fn validate_system(&self, system: &System) -> bool {
        is_field_valid(&self.arch, system.arch.as_ref())
            && is_field_valid(&self.os, system.os.as_ref())
            && is_field_valid(&self.libc, system.libc.as_ref())
    }

    /// Whether the requirements are satisfied by at least one of the given
    /// sets. Each set is checked as a whole, so listing two sets is *not*
    /// the same as merging their fields together into a single one.
    pub fn validate_any(&self, sets: &[SystemSet]) -> bool {
        sets.iter().any(|set| self.validate_set(set))
    }

    /// Whether at least one system of the set satisfies the requirements.
    /// Since the set is the cross product of its fields, each field can be
    /// checked independently.
    pub fn validate_set(&self, set: &SystemSet) -> bool {
        is_field_valid_for_set(&self.arch, &set.arch)
            && is_field_valid_for_set(&self.os, &set.os)
            && is_field_valid_for_set(&self.libc, &set.libc)
    }
}

#[cfg(test)]
mod requirements_tests {
    use super::*;

    #[test]
    fn extend_accepts_the_systems_of_both_requirements() {
        let mut requirements
            = System::new(Some(Cpu::Aarch64), Some(Os::MacOS), None).to_requirements();

        requirements.extend(&System::new(Some(Cpu::X86_64), Some(Os::MacOS), None).to_requirements());

        assert!(requirements.validate_system(&System::new(Some(Cpu::Aarch64), Some(Os::MacOS), None)));
        assert!(requirements.validate_system(&System::new(Some(Cpu::X86_64), Some(Os::MacOS), None)));
        assert!(!requirements.validate_system(&System::new(Some(Cpu::X86_64), Some(Os::Linux), None)));
    }

    fn parse(input: &str) -> Requirements {
        serde_json::from_str(input).unwrap()
    }

    #[test]
    fn deserialize_accepts_strings_as_single_element_lists() {
        assert_eq!(parse(r#"{"os": "linux", "cpu": "x64", "libc": "musl"}"#), parse(r#"{"os": ["linux"], "cpu": ["x64"], "libc": ["musl"]}"#));
        assert_eq!(parse(r#"{"os": "!win32"}"#), parse(r#"{"os": ["!win32"]}"#));
    }

    #[test]
    fn deserialize_treats_null_as_absent() {
        assert_eq!(parse(r#"{"os": null, "cpu": null, "libc": null}"#), Requirements::default());
    }

    #[test]
    fn deserialize_still_rejects_other_shapes() {
        assert!(serde_json::from_str::<Requirements>(r#"{"libc": 42}"#).is_err());
        assert!(serde_json::from_str::<Requirements>(r#"{"libc": {"musl": true}}"#).is_err());
        assert!(serde_json::from_str::<Requirements>(r#"{"libc": ["musl", 42]}"#).is_err());
    }

    #[test]
    fn deserialize_works_when_flattened() {
        #[derive(Deserialize)]
        struct Manifest {
            #[allow(dead_code)]
            name: String,

            #[serde(flatten)]
            requirements: Requirements,
        }

        let manifest: Manifest
            = serde_json::from_str(r#"{"name": "sass-embedded-linux-musl-x64", "os": ["linux"], "cpu": "x64", "libc": "musl"}"#).unwrap();

        assert_eq!(manifest.requirements, parse(r#"{"os": ["linux"], "cpu": ["x64"], "libc": ["musl"]}"#));
    }

    #[test]
    fn string_libc_is_validated_like_a_list() {
        let requirements
            = parse(r#"{"os": "linux", "libc": "musl"}"#);

        assert!(requirements.validate_system(&System::new(Some(Cpu::X86_64), Some(Os::Linux), Some(Libc::Musl))));
        assert!(!requirements.validate_system(&System::new(Some(Cpu::X86_64), Some(Os::Linux), Some(Libc::Glibc))));
        assert!(!requirements.validate_system(&System::new(Some(Cpu::X86_64), Some(Os::Linux), None)));
    }

    #[test]
    fn serialize_always_emits_lists() {
        let serialized
            = serde_json::to_string(&parse(r#"{"libc": "musl"}"#)).unwrap();

        assert_eq!(serialized, r#"{"libc":["musl"]}"#);
    }

    fn os_system(os: Os) -> System {
        System::new(Some(Cpu::X86_64), Some(os), Some(Libc::Glibc))
    }

    #[test]
    fn validate_system_follows_npm_semantics() {
        // Expected compatibility with linux, darwin, and win32 respectively
        let truth_table: &[(&str, [bool; 3])] = &[
            (r#"[]"#,                      [true,  true,  true ]),
            (r#"["linux"]"#,               [true,  false, false]),
            (r#"["linux", "darwin"]"#,     [true,  true,  false]),
            (r#"["!win32"]"#,              [true,  true,  false]),
            (r#"["!win32", "!darwin"]"#,   [true,  false, false]),
            (r#"["linux", "!win32"]"#,     [true,  false, false]),
            (r#"["linux", "!linux"]"#,     [false, false, false]),
            (r#"["!linux", "linux"]"#,     [false, false, false]),
            (r#"["any"]"#,                 [true,  true,  true ]),
            (r#"["any", "!win32"]"#,       [false, false, false]),
            (r#"["!!win32"]"#,             [true,  true,  true ]),
            (r#"["foo"]"#,                 [false, false, false]),
            (r#"["!foo"]"#,                [true,  true,  true ]),
        ];

        for (list, expected) in truth_table {
            let requirements
                = parse(&format!(r#"{{"os": {}}}"#, list));

            let actual = [Os::Linux, Os::MacOS, Os::Windows]
                .map(|os| requirements.validate_system(&os_system(os)));

            assert_eq!(&actual, expected, "os: {}", list);
        }
    }

    #[test]
    fn validate_system_supports_negations_in_every_field() {
        let requirements
            = parse(r#"{"os": ["!win32"], "cpu": ["!ia32", "!arm64"], "libc": ["!musl"]}"#);

        assert!(requirements.validate_system(&System::new(Some(Cpu::X86_64), Some(Os::Linux), Some(Libc::Glibc))));
        assert!(!requirements.validate_system(&System::new(Some(Cpu::X86_64), Some(Os::Windows), Some(Libc::Glibc))));
        assert!(!requirements.validate_system(&System::new(Some(Cpu::I386), Some(Os::Linux), Some(Libc::Glibc))));
        assert!(!requirements.validate_system(&System::new(Some(Cpu::Aarch64), Some(Os::Linux), Some(Libc::Glibc))));
        assert!(!requirements.validate_system(&System::new(Some(Cpu::X86_64), Some(Os::Linux), Some(Libc::Musl))));
    }

    #[test]
    fn validate_system_rejects_unknown_libcs_like_npm() {
        let darwin
            = System::new(Some(Cpu::Aarch64), Some(Os::MacOS), None);

        assert!(parse(r#"{}"#).validate_system(&darwin));
        assert!(!parse(r#"{"libc": ["glibc"]}"#).validate_system(&darwin));
        assert!(!parse(r#"{"libc": ["!musl"]}"#).validate_system(&darwin));
        assert!(!parse(r#"{"libc": ["any"]}"#).validate_system(&darwin));
    }

    #[test]
    fn validate_set_accepts_requirements_matching_one_of_the_systems() {
        let set = |os: Option<Vec<Os>>, libc: Option<Vec<Libc>>| SystemSet {
            arch: Some(vec![Cpu::X86_64]),
            os,
            libc,
        };

        let not_win32
            = parse(r#"{"os": ["!win32"]}"#);

        assert!(not_win32.validate_set(&set(Some(vec![Os::Linux, Os::Windows]), None)));
        assert!(not_win32.validate_set(&set(None, None)));
        assert!(!not_win32.validate_set(&set(Some(vec![Os::Windows]), None)));
        assert!(!not_win32.validate_set(&set(Some(vec![]), None)));

        let mixed
            = parse(r#"{"os": ["linux", "!win32"]}"#);

        assert!(mixed.validate_set(&set(Some(vec![Os::Linux, Os::Windows]), None)));
        assert!(!mixed.validate_set(&set(Some(vec![Os::MacOS, Os::Windows]), None)));
        assert!(mixed.validate_set(&set(None, None)));

        // A list that accepts no value at all can't be satisfied, even when
        // all values are supported
        assert!(!parse(r#"{"os": ["linux", "!linux"]}"#).validate_set(&set(None, None)));

        let not_musl
            = parse(r#"{"libc": ["!musl"]}"#);

        assert!(not_musl.validate_set(&set(None, Some(vec![Libc::Glibc, Libc::Musl]))));
        assert!(!not_musl.validate_set(&set(None, Some(vec![Libc::Musl]))));
        assert!(!not_musl.validate_set(&set(None, Some(vec![]))));
        assert!(not_musl.validate_set(&set(None, None)));
    }

    #[test]
    fn extend_unions_negated_lists() {
        let extend = |a: &str, b: &str| {
            let mut requirements
                = parse(&format!(r#"{{"os": {}}}"#, a));

            requirements.extend(&parse(&format!(r#"{{"os": {}}}"#, b)));
            requirements
        };

        assert_eq!(extend(r#"["!win32"]"#, r#"["win32"]"#), parse(r#"{}"#));
        assert_eq!(extend(r#"["!win32"]"#, r#"["!darwin"]"#), parse(r#"{}"#));
        assert_eq!(extend(r#"["!win32", "!darwin"]"#, r#"["!win32"]"#), parse(r#"{"os": ["!win32"]}"#));
        assert_eq!(extend(r#"["!win32", "!darwin"]"#, r#"["darwin"]"#), parse(r#"{"os": ["!win32"]}"#));
        assert_eq!(extend(r#"["linux"]"#, r#"["!win32"]"#), parse(r#"{"os": ["!win32"]}"#));
        assert_eq!(extend(r#"["linux", "!win32"]"#, r#"["darwin"]"#), parse(r#"{"os": ["linux", "darwin"]}"#));
        assert_eq!(extend(r#"["linux"]"#, r#"["any"]"#), parse(r#"{}"#));
        assert_eq!(extend(r#"["linux", "!linux"]"#, r#"["darwin", "!darwin"]"#), parse(r#"{"os": ["linux", "!linux"]}"#));
    }

    #[test]
    fn extend_accepts_the_systems_of_both_negated_requirements() {
        let systems
            = [Os::Linux, Os::MacOS, Os::Windows, Os::Other("aix".to_string())];

        let lists
            = [r#"["linux"]"#, r#"["!win32"]"#, r#"["!win32", "!darwin"]"#, r#"["linux", "!win32"]"#, r#"["darwin", "win32"]"#, r#"["win32"]"#];

        for a in lists {
            for b in lists {
                let first
                    = parse(&format!(r#"{{"os": {}}}"#, a));
                let second
                    = parse(&format!(r#"{{"os": {}}}"#, b));

                let mut union
                    = first.clone();

                union.extend(&second);

                for os in &systems {
                    let system
                        = os_system(os.clone());

                    assert_eq!(union.validate_system(&system), first.validate_system(&system) || second.validate_system(&system), "{} | {} on {:?}", a, b, os);
                }
            }
        }
    }

    #[test]
    fn negated_entries_round_trip() {
        let input
            = r#"{"cpu":["!ia32","x64"],"os":["!win32"],"libc":["!musl","!!glibc"]}"#;

        assert_eq!(serde_json::to_string(&parse(input)).unwrap(), input);
        assert_eq!(parse(input).os, vec![RequirementEntry::Exclude(Os::Windows)]);
    }

    #[test]
    fn positive_entries_keep_their_format() {
        let input
            = r#"{"cpu":["x64"],"os":["linux","foo"],"libc":["glibc"]}"#;

        assert_eq!(serde_json::to_string(&parse(input)).unwrap(), input);
        assert_eq!(parse(input).os, vec![RequirementEntry::Include(Os::Linux), RequirementEntry::Include(Os::Other("foo".to_string()))]);
    }
}
