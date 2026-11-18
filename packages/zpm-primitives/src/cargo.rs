//! Cargo flavored semver.
//!
//! Cargo doesn't interpret versions and requirements the way npm does: a
//! bare `1.2` means `^1.2`, comparators are joined by commas, build metadata
//! is part of a version's identity (`0.11.1+wasi-snapshot-preview1` is the
//! name of the file to download), and pre-releases only match requirements
//! that mention a pre-release of the very same `major.minor.patch`. The
//! types below follow the `semver` crate, which is what Cargo uses.

use std::{cmp::Ordering, fmt};

use rkyv::Archive;
use zpm_semver::VersionRc;
use zpm_utils::{DataType, EcoVec, FromFileString, ToFileString, ToHumanString, impl_file_string_from_str, impl_file_string_serialization};

#[derive(thiserror::Error, Clone, Debug, PartialEq, Eq)]
pub enum CargoError {
    #[error("Invalid Cargo version: {0}")]
    InvalidVersion(String),

    #[error("Invalid Cargo version requirement: {0}")]
    InvalidRequirement(String),

    #[error("Invalid Cargo git source: {0}")]
    InvalidGitSource(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum CargoPrerelease {
    Numeric(u64),
    Alpha(String),
}

impl PartialOrd for CargoPrerelease {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for CargoPrerelease {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (CargoPrerelease::Numeric(a), CargoPrerelease::Numeric(b)) => a.cmp(b),
            (CargoPrerelease::Numeric(_), CargoPrerelease::Alpha(_)) => Ordering::Less,
            (CargoPrerelease::Alpha(_), CargoPrerelease::Numeric(_)) => Ordering::Greater,
            (CargoPrerelease::Alpha(a), CargoPrerelease::Alpha(b)) => a.cmp(b),
        }
    }
}

fn parse_identifiers(src: &str) -> Option<Vec<CargoPrerelease>> {
    src.split('.').map(|segment| {
        if segment.is_empty() || !segment.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-') {
            return None;
        }

        if segment.bytes().all(|c| c.is_ascii_digit()) {
            if let Ok(number) = segment.parse::<u64>() {
                return Some(CargoPrerelease::Numeric(number));
            }
        }

        Some(CargoPrerelease::Alpha(segment.to_string()))
    }).collect()
}

/// A pre-release sorts before the release it precedes.
fn cmp_prerelease(a: &[CargoPrerelease], b: &[CargoPrerelease]) -> Ordering {
    match (a.is_empty(), b.is_empty()) {
        (true, true) => Ordering::Equal,
        (true, false) => Ordering::Greater,
        (false, true) => Ordering::Less,
        (false, false) => a.cmp(b),
    }
}

/// The set of versions Cargo considers interchangeable: a dependency
/// graph may hold several versions of a crate, but only one per bucket.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CargoBucket {
    Major(u64),
    Minor(u64),
    Patch(u64),
}

impl fmt::Display for CargoBucket {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CargoBucket::Major(major) => write!(f, "{}.x", major),
            CargoBucket::Minor(minor) => write!(f, "0.{}.x", minor),
            CargoBucket::Patch(patch) => write!(f, "0.0.{}", patch),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CargoSemver {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    pub pre: Vec<CargoPrerelease>,
    pub build: String,
}

impl CargoSemver {
    pub fn parse(src: &str) -> Option<CargoSemver> {
        let (src, build) = match src.split_once('+') {
            Some((src, build)) => (src, build),
            None => (src, ""),
        };

        if !build.is_empty() {
            parse_identifiers(build)?;
        }

        let (numbers, pre) = match src.split_once('-') {
            Some((numbers, pre)) => (numbers, parse_identifiers(pre)?),
            None => (src, Vec::new()),
        };

        let mut components
            = numbers.split('.');

        let mut next_component = || -> Option<u64> {
            let component
                = components.next()?;

            if component.is_empty() || !component.bytes().all(|c| c.is_ascii_digit()) {
                return None;
            }

            component.parse().ok()
        };

        let major = next_component()?;
        let minor = next_component()?;
        let patch = next_component()?;

        if components.next().is_some() {
            return None;
        }

        Some(CargoSemver {
            major,
            minor,
            patch,
            pre,
            build: build.to_string(),
        })
    }

    pub fn is_prerelease(&self) -> bool {
        !self.pre.is_empty()
    }

    pub fn bucket(&self) -> CargoBucket {
        if self.major > 0 {
            CargoBucket::Major(self.major)
        } else if self.minor > 0 {
            CargoBucket::Minor(self.minor)
        } else {
            CargoBucket::Patch(self.patch)
        }
    }

    /// The lowest version of a bucket; only used to give buckets a
    /// position on the version line.
    pub fn bucket_floor(bucket: &CargoBucket) -> CargoSemver {
        let (major, minor, patch) = match bucket {
            CargoBucket::Major(major) => (*major, 0, 0),
            CargoBucket::Minor(minor) => (0, *minor, 0),
            CargoBucket::Patch(patch) => (0, 0, *patch),
        };

        CargoSemver {major, minor, patch, pre: Vec::new(), build: String::new()}
    }
}

fn cmp_build(a: &str, b: &str) -> Ordering {
    let mut lhs = a.split('.').filter(|segment| !segment.is_empty());
    let mut rhs = b.split('.').filter(|segment| !segment.is_empty());

    loop {
        match (lhs.next(), rhs.next()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,

            (Some(lhs), Some(rhs)) => {
                let ordering = match (lhs.parse::<u64>(), rhs.parse::<u64>()) {
                    (Ok(lhs), Ok(rhs)) => lhs.cmp(&rhs),
                    (Ok(_), Err(_)) => Ordering::Less,
                    (Err(_), Ok(_)) => Ordering::Greater,
                    (Err(_), Err(_)) => lhs.cmp(rhs),
                };

                if ordering != Ordering::Equal {
                    return ordering;
                }
            },
        }
    }
}

impl PartialOrd for CargoSemver {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for CargoSemver {
    fn cmp(&self, other: &Self) -> Ordering {
        self.major.cmp(&other.major)
            .then_with(|| self.minor.cmp(&other.minor))
            .then_with(|| self.patch.cmp(&other.patch))
            .then_with(|| cmp_prerelease(&self.pre, &other.pre))
            .then_with(|| cmp_build(&self.build, &other.build))
    }
}

impl fmt::Display for CargoSemver {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;

        for (index, identifier) in self.pre.iter().enumerate() {
            f.write_str(if index == 0 {"-"} else {"."})?;

            match identifier {
                CargoPrerelease::Numeric(number) => write!(f, "{}", number)?,
                CargoPrerelease::Alpha(value) => f.write_str(value)?,
            }
        }

        if !self.build.is_empty() {
            write!(f, "+{}", self.build)?;
        }

        Ok(())
    }
}

/// The version of a crate, as written in the index and in lockfiles.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Archive, rkyv::Serialize, rkyv::Deserialize)]
#[rkyv(derive(PartialEq, Eq, PartialOrd, Ord, Hash))]
pub struct CargoVersion {
    raw: String,
}

impl CargoVersion {
    pub fn as_str(&self) -> &str {
        &self.raw
    }

    pub fn to_semver(&self) -> CargoSemver {
        CargoSemver::parse(&self.raw)
            .expect("Cargo versions are validated when they're created")
    }

    /// Projection on Yarn's own version type, which has no build metadata.
    pub fn to_lossy_semver(&self) -> zpm_semver::Version {
        let parsed
            = self.to_semver();

        let rc = (!parsed.pre.is_empty()).then(|| {
            EcoVec::from(parsed.pre.iter().map(|identifier| match identifier {
                CargoPrerelease::Numeric(number) if *number <= zpm_semver::MAX_SAFE_INTEGER => VersionRc::Number(*number),
                CargoPrerelease::Numeric(number) => VersionRc::String(number.to_string().into()),
                CargoPrerelease::Alpha(value) => VersionRc::String(value.as_str().into()),
            }).collect::<Vec<_>>())
        });

        let clamp
            = |n: u64| n.min(zpm_semver::MAX_SAFE_INTEGER);

        zpm_semver::Version::new_from_components(clamp(parsed.major), clamp(parsed.minor), clamp(parsed.patch), rc)
    }
}

impl From<&CargoSemver> for CargoVersion {
    fn from(version: &CargoSemver) -> Self {
        Self {raw: version.to_string()}
    }
}

impl FromFileString for CargoVersion {
    type Error = CargoError;

    fn from_file_string(src: &str) -> Result<Self, Self::Error> {
        let src
            = src.trim();

        CargoSemver::parse(src)
            .map(|_| Self {raw: src.to_string()})
            .ok_or_else(|| CargoError::InvalidVersion(src.to_string()))
    }
}

impl ToFileString for CargoVersion {
    fn to_file_string(&self) -> String {
        self.raw.clone()
    }
}

impl ToHumanString for CargoVersion {
    fn to_print_string(&self) -> String {
        DataType::Reference.colorize(&self.raw)
    }
}

impl_file_string_from_str!(CargoVersion);
impl_file_string_serialization!(CargoVersion);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CargoOperator {
    Exact,
    Greater,
    GreaterEq,
    Less,
    LessEq,
    Tilde,
    Caret,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CargoComparator {
    operator: CargoOperator,
    major: u64,
    minor: Option<u64>,
    patch: Option<u64>,
    pre: Vec<CargoPrerelease>,
}

fn is_wildcard(component: &str) -> bool {
    matches!(component, "*" | "x" | "X")
}

impl CargoComparator {
    /// Returns `Ok(None)` for the comparators matching everything (`*`).
    fn parse(src: &str) -> Option<Option<CargoComparator>> {
        let src
            = src.trim();

        let (operator, has_operator, rest) = if let Some(rest) = src.strip_prefix(">=") {
            (CargoOperator::GreaterEq, true, rest)
        } else if let Some(rest) = src.strip_prefix("<=") {
            (CargoOperator::LessEq, true, rest)
        } else if let Some(rest) = src.strip_prefix('>') {
            (CargoOperator::Greater, true, rest)
        } else if let Some(rest) = src.strip_prefix('<') {
            (CargoOperator::Less, true, rest)
        } else if let Some(rest) = src.strip_prefix('=') {
            (CargoOperator::Exact, true, rest)
        } else if let Some(rest) = src.strip_prefix('~') {
            (CargoOperator::Tilde, true, rest)
        } else if let Some(rest) = src.strip_prefix('^') {
            (CargoOperator::Caret, true, rest)
        } else {
            (CargoOperator::Caret, false, src)
        };

        let rest
            = rest.trim();

        // Build metadata is accepted and ignored in requirements
        let rest
            = rest.split_once('+').map_or(rest, |(rest, _)| rest);

        let (numbers, pre) = match rest.split_once('-') {
            Some((numbers, pre)) => (numbers, parse_identifiers(pre)?),
            None => (rest, Vec::new()),
        };

        let components
            = numbers.split('.').collect::<Vec<_>>();

        if components.is_empty() || components.len() > 3 {
            return None;
        }

        let mut parsed
            = Vec::with_capacity(3);

        let mut has_wildcard
            = false;

        for component in &components {
            if is_wildcard(component) {
                has_wildcard = true;
                continue;
            }

            // Nothing concrete may follow a wildcard (`1.*.3`)
            if has_wildcard || component.is_empty() || !component.bytes().all(|c| c.is_ascii_digit()) {
                return None;
            }

            parsed.push(component.parse::<u64>().ok()?);
        }

        if has_wildcard {
            if !pre.is_empty() {
                return None;
            }

            // `*`, `>=*`... all match everything; `1.*` is `=1`
            let Some(major) = parsed.first().copied() else {
                return Some(None);
            };

            return Some(Some(CargoComparator {
                operator: if has_operator {operator} else {CargoOperator::Exact},
                major,
                minor: parsed.get(1).copied(),
                patch: None,
                pre,
            }));
        }

        // A pre-release can only be attached to a full version
        if !pre.is_empty() && parsed.len() != 3 {
            return None;
        }

        Some(Some(CargoComparator {
            operator,
            major: *parsed.first()?,
            minor: parsed.get(1).copied(),
            patch: parsed.get(2).copied(),
            pre,
        }))
    }

    fn matches_exact(&self, version: &CargoSemver) -> bool {
        if version.major != self.major {
            return false;
        }

        match self.minor {
            None => return true,
            Some(minor) if version.minor != minor => return false,
            Some(_) => {},
        }

        match self.patch {
            None => return true,
            Some(patch) if version.patch != patch => return false,
            Some(_) => {},
        }

        version.pre == self.pre
    }

    fn matches_greater(&self, version: &CargoSemver) -> bool {
        if version.major != self.major {
            return version.major > self.major;
        }

        match self.minor {
            None => return false,
            Some(minor) if version.minor != minor => return version.minor > minor,
            Some(_) => {},
        }

        match self.patch {
            None => return false,
            Some(patch) if version.patch != patch => return version.patch > patch,
            Some(_) => {},
        }

        cmp_prerelease(&version.pre, &self.pre) == Ordering::Greater
    }

    fn matches_less(&self, version: &CargoSemver) -> bool {
        if version.major != self.major {
            return version.major < self.major;
        }

        match self.minor {
            None => return false,
            Some(minor) if version.minor != minor => return version.minor < minor,
            Some(_) => {},
        }

        match self.patch {
            None => return false,
            Some(patch) if version.patch != patch => return version.patch < patch,
            Some(_) => {},
        }

        cmp_prerelease(&version.pre, &self.pre) == Ordering::Less
    }

    fn matches_tilde(&self, version: &CargoSemver) -> bool {
        if version.major != self.major {
            return false;
        }

        if let Some(minor) = self.minor {
            if version.minor != minor {
                return false;
            }
        }

        if let Some(patch) = self.patch {
            if version.patch != patch {
                return version.patch > patch;
            }
        }

        cmp_prerelease(&version.pre, &self.pre) != Ordering::Less
    }

    fn matches_caret(&self, version: &CargoSemver) -> bool {
        if version.major != self.major {
            return false;
        }

        let Some(minor) = self.minor else {
            return true;
        };

        let Some(patch) = self.patch else {
            return match self.major > 0 {
                true => version.minor >= minor,
                false => version.minor == minor,
            };
        };

        if self.major > 0 {
            if version.minor != minor {
                return version.minor > minor;
            } else if version.patch != patch {
                return version.patch > patch;
            }
        } else if minor > 0 {
            if version.minor != minor {
                return false;
            } else if version.patch != patch {
                return version.patch > patch;
            }
        } else if version.minor != minor || version.patch != patch {
            return false;
        }

        cmp_prerelease(&version.pre, &self.pre) != Ordering::Less
    }

    fn matches(&self, version: &CargoSemver) -> bool {
        match self.operator {
            CargoOperator::Exact => self.matches_exact(version),
            CargoOperator::Greater => self.matches_greater(version),
            CargoOperator::GreaterEq => self.matches_exact(version) || self.matches_greater(version),
            CargoOperator::Less => self.matches_less(version),
            CargoOperator::LessEq => self.matches_exact(version) || self.matches_less(version),
            CargoOperator::Tilde => self.matches_tilde(version),
            CargoOperator::Caret => self.matches_caret(version),
        }
    }

    /// Pre-releases are opt-in: one only matches when the requirement
    /// mentions a pre-release of the same `major.minor.patch`.
    fn allows_prerelease_of(&self, version: &CargoSemver) -> bool {
        !self.pre.is_empty()
            && self.major == version.major
            && self.minor == Some(version.minor)
            && self.patch == Some(version.patch)
    }
}

/// A parsed version requirement (`^1.2`, `>=1, <3`, `=0.8.0-rc.1`, `*`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CargoReq {
    comparators: Vec<CargoComparator>,
}

impl CargoReq {
    pub fn any() -> CargoReq {
        CargoReq {comparators: Vec::new()}
    }

    pub fn parse(src: &str) -> Option<CargoReq> {
        let src
            = src.trim();

        if src.is_empty() {
            return None;
        }

        let mut comparators
            = Vec::new();

        for segment in src.split(',') {
            if let Some(comparator) = CargoComparator::parse(segment)? {
                comparators.push(comparator);
            }
        }

        Some(CargoReq {comparators})
    }

    pub fn exact(version: &CargoSemver) -> CargoReq {
        CargoReq {
            comparators: vec![CargoComparator {
                operator: CargoOperator::Exact,
                major: version.major,
                minor: Some(version.minor),
                patch: Some(version.patch),
                pre: version.pre.clone(),
            }],
        }
    }

    pub fn matches(&self, version: &CargoSemver) -> bool {
        if !self.comparators.iter().all(|comparator| comparator.matches(version)) {
            return false;
        }

        version.pre.is_empty() || self.comparators.iter().any(|comparator| comparator.allows_prerelease_of(version))
    }
}

/// A version requirement as written in manifests, minus its whitespace
/// (`", "` separates the keys of a lockfile entry).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Archive, rkyv::Serialize, rkyv::Deserialize)]
#[rkyv(derive(PartialEq, Eq, PartialOrd, Ord, Hash))]
pub struct CargoRequirement {
    raw: String,
}

impl CargoRequirement {
    pub fn any() -> Self {
        Self {raw: "*".to_string()}
    }

    pub fn as_str(&self) -> &str {
        &self.raw
    }

    pub fn to_req(&self) -> CargoReq {
        CargoReq::parse(&self.raw)
            .expect("Cargo requirements are validated when they're created")
    }
}

impl FromFileString for CargoRequirement {
    type Error = CargoError;

    fn from_file_string(src: &str) -> Result<Self, Self::Error> {
        let raw
            = src.chars().filter(|c| !c.is_whitespace()).collect::<String>();

        CargoReq::parse(&raw)
            .map(|_| Self {raw})
            .ok_or_else(|| CargoError::InvalidRequirement(src.to_string()))
    }
}

impl ToFileString for CargoRequirement {
    fn to_file_string(&self) -> String {
        self.raw.clone()
    }
}

impl ToHumanString for CargoRequirement {
    fn to_print_string(&self) -> String {
        DataType::Range.colorize(&self.raw)
    }
}

impl_file_string_from_str!(CargoRequirement);
impl_file_string_serialization!(CargoRequirement);

/// Same encoding as the query strings Cargo writes in its lockfiles
/// (`application/x-www-form-urlencoded`).
fn encode_query_value(value: &str) -> String {
    let mut encoded
        = String::with_capacity(value.len());

    for byte in value.bytes() {
        match byte {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'*' => encoded.push(byte as char),
            b' ' => encoded.push('+'),
            _ => encoded.push_str(&format!("%{:02X}", byte)),
        }
    }

    encoded
}

fn decode_query_value(value: &str) -> String {
    let bytes
        = value.as_bytes();

    let mut decoded
        = Vec::with_capacity(bytes.len());

    let mut index
        = 0;

    while index < bytes.len() {
        let escaped = (bytes[index] == b'%')
            .then(|| value.get(index + 1..index + 3))
            .flatten()
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());

        match (escaped, bytes[index]) {
            (Some(byte), _) => {
                decoded.push(byte);
                index += 3;
            },

            (None, b'+') => {
                decoded.push(b' ');
                index += 1;
            },

            (None, byte) => {
                decoded.push(byte);
                index += 1;
            },
        }
    }

    String::from_utf8_lossy(&decoded).to_string()
}

/// What a git dependency asks for.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CargoGitRef {
    DefaultBranch,
    Branch(String),
    Tag(String),
    Rev(String),
}

/// A git repository and the ref to take from it, written the way Cargo
/// writes its source ids: `<url>`, `<url>?branch=<name>`, `<url>?tag=<name>`,
/// or `<url>?rev=<rev>`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Archive, rkyv::Serialize, rkyv::Deserialize)]
#[rkyv(derive(PartialEq, Eq, PartialOrd, Ord, Hash))]
pub struct CargoGitSource {
    raw: String,
}

impl CargoGitSource {
    pub fn new(url: &str, git_ref: &CargoGitRef) -> Self {
        let query = match git_ref {
            CargoGitRef::DefaultBranch => String::new(),
            CargoGitRef::Branch(branch) => format!("?branch={}", encode_query_value(branch)),
            CargoGitRef::Tag(tag) => format!("?tag={}", encode_query_value(tag)),
            CargoGitRef::Rev(rev) => format!("?rev={}", encode_query_value(rev)),
        };

        Self {raw: format!("{}{}", url, query)}
    }

    pub fn as_str(&self) -> &str {
        &self.raw
    }

    pub fn url(&self) -> &str {
        self.raw.split_once('?').map_or(self.raw.as_str(), |(url, _)| url)
    }

    pub fn git_ref(&self) -> CargoGitRef {
        let Some((_, query)) = self.raw.split_once('?') else {
            return CargoGitRef::DefaultBranch;
        };

        for pair in query.split('&') {
            let Some((key, value)) = pair.split_once('=') else {
                continue;
            };

            let value
                = decode_query_value(value);

            match key {
                "branch" => return CargoGitRef::Branch(value),
                "tag" => return CargoGitRef::Tag(value),
                "rev" => return CargoGitRef::Rev(value),
                _ => {},
            }
        }

        CargoGitRef::DefaultBranch
    }
}

impl FromFileString for CargoGitSource {
    type Error = CargoError;

    fn from_file_string(src: &str) -> Result<Self, Self::Error> {
        if src.is_empty() || src.contains('#') || src.chars().any(|c| c.is_whitespace()) {
            return Err(CargoError::InvalidGitSource(src.to_string()));
        }

        Ok(Self {raw: src.to_string()})
    }
}

impl ToFileString for CargoGitSource {
    fn to_file_string(&self) -> String {
        self.raw.clone()
    }
}

impl ToHumanString for CargoGitSource {
    fn to_print_string(&self) -> String {
        DataType::Reference.colorize(&self.raw)
    }
}

impl_file_string_from_str!(CargoGitSource);
impl_file_string_serialization!(CargoGitSource);

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    fn version(src: &str) -> CargoSemver {
        CargoSemver::parse(src).unwrap()
    }

    #[rstest]
    #[case("1.2.3", "1.2.3", true)]
    #[case("1.2.3", "1.9.0", true)]
    #[case("1.2.3", "2.0.0", false)]
    #[case("1.2.3", "1.2.2", false)]
    #[case("1.2", "1.2.0", true)]
    #[case("1", "1.0.0", true)]
    #[case("1", "2.0.0", false)]
    #[case("0.2.3", "0.2.9", true)]
    #[case("0.2.3", "0.3.0", false)]
    #[case("0.0.3", "0.0.3", true)]
    #[case("0.0.3", "0.0.4", false)]
    #[case("0.0", "0.0.9", true)]
    #[case("0", "0.9.0", true)]
    #[case("^1.2.3", "1.3.0", true)]
    #[case("~1.2.3", "1.2.9", true)]
    #[case("~1.2.3", "1.3.0", false)]
    #[case("~1.2", "1.2.9", true)]
    #[case("~1", "1.9.0", true)]
    #[case("=1.2.3", "1.2.3", true)]
    #[case("=1.2.3", "1.2.4", false)]
    #[case("=1.2", "1.2.4", true)]
    #[case("=1", "1.9.4", true)]
    #[case(">1.2.3", "1.2.4", true)]
    #[case(">1.2.3", "1.2.3", false)]
    #[case(">1.2", "1.2.9", false)]
    #[case(">1.2", "1.3.0", true)]
    #[case(">=1.2", "1.2.0", true)]
    #[case("<1.2", "1.1.9", true)]
    #[case("<1.2", "1.2.0", false)]
    #[case("<=1.2", "1.2.9", true)]
    #[case("<=1.2", "1.3.0", false)]
    #[case(">=1.2.3, <1.8.0", "1.5.0", true)]
    #[case(">=1.2.3, <1.8.0", "1.8.0", false)]
    #[case(">=0.6,<0.10", "0.9.3", true)]
    #[case("*", "3.1.4", true)]
    #[case("1.*", "1.9.9", true)]
    #[case("1.*", "2.0.0", false)]
    #[case("1.2.*", "1.2.9", true)]
    #[case("1.2.*", "1.3.0", false)]
    // Build metadata never takes part in the matching
    #[case("0.11", "0.11.1+wasi-snapshot-preview1", true)]
    #[case("1.0.0+ignored", "1.0.1", true)]
    // Pre-releases must be asked for, on the very same version
    #[case("1", "1.5.0-beta.1", false)]
    #[case("*", "1.5.0-beta.1", false)]
    #[case("<2.0.0", "2.0.0-alpha.1", false)]
    #[case("1.0.0-alpha.8", "1.0.0-alpha.8", true)]
    #[case("1.0.0-alpha.8", "1.0.0-alpha.9", true)]
    #[case("1.0.0-alpha.8", "1.0.0-alpha.7", false)]
    #[case("1.0.0-alpha.8", "1.0.0", true)]
    #[case("1.0.0-alpha.8", "1.0.1-alpha.1", false)]
    #[case("=6.0.0-alpha.0", "6.0.0-alpha.0", true)]
    #[case("=6.0.0-alpha.0", "6.0.0", false)]
    fn test_requirement_matching(#[case] requirement: &str, #[case] candidate: &str, #[case] expected: bool) {
        let requirement
            = CargoRequirement::from_file_string(requirement).unwrap();

        assert_eq!(requirement.to_req().matches(&version(candidate)), expected);
    }

    #[rstest]
    #[case("")]
    #[case("latest")]
    #[case("1.2.3.4")]
    #[case("1.*.3")]
    #[case("1.2-beta")]
    #[case(">=")]
    fn test_invalid_requirements(#[case] requirement: &str) {
        assert!(CargoRequirement::from_file_string(requirement).is_err());
    }

    #[test]
    fn test_requirements_lose_their_whitespace() {
        assert_eq!(CargoRequirement::from_file_string(">=1.2.3, <1.8.0").unwrap().as_str(), ">=1.2.3,<1.8.0");
    }

    #[test]
    fn test_version_ordering() {
        let mut versions
            = ["1.0.0", "1.0.0-alpha.8", "1.0.0-alpha.10", "1.0.0-alpha", "0.9.34+deprecated", "0.10.0", "1.0.0-beta", "1.0.0+b"].map(version).to_vec();

        versions.sort();

        assert_eq!(versions.iter().map(|version| version.to_string()).collect::<Vec<_>>(), vec![
            "0.9.34+deprecated",
            "0.10.0",
            "1.0.0-alpha",
            "1.0.0-alpha.8",
            "1.0.0-alpha.10",
            "1.0.0-beta",
            "1.0.0",
            "1.0.0+b",
        ]);
    }

    #[rstest]
    #[case("1.2.3", CargoBucket::Major(1))]
    #[case("1.0.0-alpha.8", CargoBucket::Major(1))]
    #[case("0.4.9", CargoBucket::Minor(4))]
    #[case("0.0.7", CargoBucket::Patch(7))]
    fn test_buckets(#[case] src: &str, #[case] expected: CargoBucket) {
        assert_eq!(version(src).bucket(), expected);
    }

    #[test]
    fn test_versions_keep_their_build_metadata() {
        let version
            = CargoVersion::from_file_string("0.11.1+wasi-snapshot-preview1").unwrap();

        assert_eq!(version.to_file_string(), "0.11.1+wasi-snapshot-preview1");
        assert_eq!(version.to_lossy_semver().to_file_string(), "0.11.1");
    }

    #[test]
    fn test_git_sources() {
        let source
            = CargoGitSource::new("https://github.com/arcanis/dialoguer.git", &CargoGitRef::Branch("mael/confirm-prompt-height".to_string()));

        assert_eq!(source.as_str(), "https://github.com/arcanis/dialoguer.git?branch=mael%2Fconfirm-prompt-height");
        assert_eq!(source.url(), "https://github.com/arcanis/dialoguer.git");
        assert_eq!(source.git_ref(), CargoGitRef::Branch("mael/confirm-prompt-height".to_string()));

        let reparsed
            = CargoGitSource::from_file_string(source.as_str()).unwrap();

        assert_eq!(reparsed, source);
        assert_eq!(CargoGitSource::from_file_string("https://example.com/repo.git").unwrap().git_ref(), CargoGitRef::DefaultBranch);
    }
}
