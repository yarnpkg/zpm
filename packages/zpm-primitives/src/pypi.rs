use std::{cmp::Ordering, collections::BTreeSet, str::FromStr};

use rkyv::Archive;
use zpm_semver::VersionRc;
use zpm_utils::{DataType, EcoVec, FromFileString, QueryString, QueryStringValue, ToFileString, ToHumanString, impl_file_string_from_str, impl_file_string_serialization};

#[derive(thiserror::Error, Clone, Debug, PartialEq, Eq)]
pub enum PypiError {
    #[error("Invalid PEP 440 version: {0}")]
    InvalidVersion(String),

    #[error("Invalid PEP 440 specifier set: {0}")]
    InvalidSpecifier(String),

    #[error("Cannot project PEP 440 version to semver: {0}")]
    InvalidSemverProjection(String),

    #[error("Invalid PyPI extra: {0}")]
    InvalidExtra(String),

    #[error("Invalid PyPI range parameter: {0}")]
    InvalidRangeParameter(String),

    #[error("Invalid PyPI range parameters: {0}")]
    InvalidRangeParameters(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Archive, rkyv::Serialize, rkyv::Deserialize)]
#[rkyv(derive(PartialEq, Eq, PartialOrd, Ord, Hash))]
pub struct PypiVersion {
    raw: String,
}

impl PypiVersion {
    pub fn as_str(&self) -> &str {
        &self.raw
    }

    pub fn is_stable(&self) -> Result<bool, PypiError> {
        Ok(self.parse()?.is_stable())
    }

    pub fn cmp_pep440(&self, other: &Self) -> Result<Ordering, PypiError> {
        Ok(self.parse()?.cmp(&other.parse()?))
    }

    pub fn satisfies(&self, specifiers: &PypiSpecifierSet) -> Result<bool, PypiError> {
        specifiers.contains(self)
    }

    pub fn to_lossy_semver(&self) -> Result<zpm_semver::Version, PypiError> {
        let parsed
            = self.parse()?;
        let release
            = parsed.release();

        let to_u32
            = |n: Option<u64>| -> Result<u32, PypiError> {
                let n
                    = n.unwrap_or(0);
            n.try_into().map_err(|_| PypiError::InvalidSemverProjection(self.raw.clone()))
            };

        let major
            = to_u32(release.first().copied())?;
        let minor
            = to_u32(release.get(1).copied())?;
        let patch
            = to_u32(release.get(2).copied())?;

        let mut prerelease_segments
            = Vec::new();

        if let Some(pre) = parsed.pre() {
            prerelease_segments.push(pre.kind.to_string());
            prerelease_segments.push(pre.number.to_string());
        }

        if let Some(dev) = parsed.dev() {
            prerelease_segments.push("dev".to_string());
            prerelease_segments.push(dev.to_string());
        }

        if let Some(post) = parsed.post() {
            prerelease_segments.push("post".to_string());
            prerelease_segments.push(post.to_string());
        }

        if !parsed.local().is_empty() {
            prerelease_segments.push("local".to_string());

            for segment in parsed.local() {
                prerelease_segments.push(segment.to_string().to_ascii_lowercase());
            }
        }

        let rc
            = if prerelease_segments.is_empty() {
                None
            } else {
                let rc_segments
                    = prerelease_segments.into_iter().map(|segment| {
                        match segment.parse::<u32>() {
                            Ok(number) => VersionRc::Number(number),
                            Err(_) => VersionRc::String(segment.into()),
                        }
                    }).collect::<Vec<_>>();

                Some(EcoVec::from(rc_segments))
            };

        Ok(zpm_semver::Version::new_from_components(major, minor, patch, rc))
    }

    fn parse(&self) -> Result<pep440_rs::Version, PypiError> {
        pep440_rs::Version::from_str(&self.raw)
            .map_err(|_| PypiError::InvalidVersion(self.raw.clone()))
    }
}

impl FromFileString for PypiVersion {
    type Error = PypiError;

    fn from_file_string(src: &str) -> Result<Self, Self::Error> {
        let src
            = src.trim();

        let parsed
            = pep440_rs::Version::from_str(src)
                .map_err(|_| PypiError::InvalidVersion(src.to_string()))?;

        Ok(Self {
            raw: parsed.to_string(),
        })
    }
}

impl ToFileString for PypiVersion {
    fn to_file_string(&self) -> String {
        self.raw.clone()
    }
}

impl ToHumanString for PypiVersion {
    fn to_print_string(&self) -> String {
        DataType::Reference.colorize(&self.raw)
    }
}

impl_file_string_from_str!(PypiVersion);
impl_file_string_serialization!(PypiVersion);

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Archive, rkyv::Serialize, rkyv::Deserialize)]
#[rkyv(derive(PartialEq, Eq, PartialOrd, Ord, Hash))]
pub struct PypiSpecifierSet {
    raw: String,
}

impl PypiSpecifierSet {
    pub fn any() -> Self {
        Self {
            raw: "*".to_string(),
        }
    }

    pub fn is_any(&self) -> bool {
        self.raw == "*"
    }

    pub fn as_str(&self) -> &str {
        &self.raw
    }

    pub fn contains(&self, version: &PypiVersion) -> Result<bool, PypiError> {
        if self.is_any() {
            return Ok(true);
        }

        let parsed_version
            = pep440_rs::Version::from_str(version.as_str())
                .map_err(|_| PypiError::InvalidVersion(version.as_str().to_string()))?;

        if let Ok(specifiers) = pep440_rs::VersionSpecifiers::from_str(&self.raw) {
            return Ok(specifiers.contains(&parsed_version));
        }

        let pinned
            = pep440_rs::Version::from_str(&self.raw)
                .map_err(|_| PypiError::InvalidSpecifier(self.raw.clone()))?;

        Ok(parsed_version == pinned)
    }
}

impl Default for PypiSpecifierSet {
    fn default() -> Self {
        Self::any()
    }
}

impl FromFileString for PypiSpecifierSet {
    type Error = PypiError;

    fn from_file_string(src: &str) -> Result<Self, Self::Error> {
        let src
            = src.trim();

        if src.is_empty() || src == "*" {
            return Ok(Self::any());
        }

        if let Ok(specifiers) = pep440_rs::VersionSpecifiers::from_str(src) {
            return Ok(Self {
                raw: specifiers.to_string(),
            });
        }

        let version
            = pep440_rs::Version::from_str(src)
                .map_err(|_| PypiError::InvalidSpecifier(src.to_string()))?;

        Ok(Self {
            raw: version.to_string(),
        })
    }
}

impl ToFileString for PypiSpecifierSet {
    fn to_file_string(&self) -> String {
        self.raw.clone()
    }
}

impl ToHumanString for PypiSpecifierSet {
    fn to_print_string(&self) -> String {
        DataType::Range.colorize(&self.raw)
    }
}

impl_file_string_from_str!(PypiSpecifierSet);
impl_file_string_serialization!(PypiSpecifierSet);

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Archive, rkyv::Serialize, rkyv::Deserialize)]
#[rkyv(derive(PartialEq, Eq, PartialOrd, Ord, Hash))]
pub struct PypiExtras {
    raw: Vec<String>,
}

impl PypiExtras {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn from_iter<I, S>(extras: I) -> Result<Self, PypiError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut unique = BTreeSet::new();

        for extra in extras {
            let extra = extra.as_ref().trim();

            if !is_valid_extra(extra) {
                return Err(PypiError::InvalidExtra(extra.to_string()));
            }

            unique.insert(extra.to_ascii_lowercase());
        }

        Ok(Self {
            raw: unique.into_iter().collect(),
        })
    }

    pub fn is_empty(&self) -> bool {
        self.raw.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &str> {
        self.raw.iter().map(|extra| extra.as_str())
    }

    pub fn contains(&self, extra: &str) -> bool {
        let extra = extra.to_ascii_lowercase();
        self.raw.iter().any(|candidate| candidate == &extra)
    }
}

fn is_valid_extra(extra: &str) -> bool {
    let mut chars = extra.chars();

    let Some(first) = chars.next() else {
        return false;
    };

    if !first.is_ascii_alphanumeric() {
        return false;
    }

    let mut previous = first;

    for ch in chars {
        if !ch.is_ascii_alphanumeric() && ch != '-' && ch != '_' && ch != '.' {
            return false;
        }

        previous = ch;
    }

    previous.is_ascii_alphanumeric()
}

impl FromFileString for PypiExtras {
    type Error = PypiError;

    fn from_file_string(src: &str) -> Result<Self, Self::Error> {
        Self::from_iter(src.split(','))
    }
}

impl ToFileString for PypiExtras {
    fn to_file_string(&self) -> String {
        self.raw.join(",")
    }
}

impl ToHumanString for PypiExtras {
    fn to_print_string(&self) -> String {
        DataType::Range.colorize(&self.to_file_string())
    }
}

impl_file_string_from_str!(PypiExtras);
impl_file_string_serialization!(PypiExtras);

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Archive, rkyv::Serialize, rkyv::Deserialize)]
#[rkyv(derive(PartialEq, Eq, PartialOrd, Ord, Hash))]
pub struct PypiRangeParameters {
    pub extras: Option<PypiExtras>,
}

impl PypiRangeParameters {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn from_extras(extras: PypiExtras) -> Self {
        Self {
            extras: (!extras.is_empty()).then_some(extras),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.extras.as_ref().map(|extras| extras.is_empty()).unwrap_or(true)
    }
}

impl FromFileString for PypiRangeParameters {
    type Error = PypiError;

    fn from_file_string(src: &str) -> Result<Self, Self::Error> {
        let query_string
            = QueryString::from_file_string(src)
                .map_err(|err| PypiError::InvalidRangeParameters(err.to_string()))?;

        let mut parameters
            = Self::empty();

        for (key, value) in query_string.fields {
            match (key.as_str(), value) {
                ("extras", QueryStringValue::String(value)) => {
                    parameters.extras = Some(PypiExtras::from_file_string(&value)?);
                },

                ("extras", QueryStringValue::True) => {
                    return Err(PypiError::InvalidRangeParameter(key));
                },

                _ => {
                    return Err(PypiError::InvalidRangeParameter(key));
                },
            }
        }

        Ok(parameters)
    }
}

impl ToFileString for PypiRangeParameters {
    fn to_file_string(&self) -> String {
        let mut parameters
            = Vec::new();

        if let Some(extras) = &self.extras {
            if !extras.is_empty() {
                parameters.push(format!("extras={}", extras.to_file_string()));
            }
        }

        parameters.join("&")
    }
}

impl ToHumanString for PypiRangeParameters {
    fn to_print_string(&self) -> String {
        DataType::Range.colorize(&self.to_file_string())
    }
}

impl_file_string_from_str!(PypiRangeParameters);
impl_file_string_serialization!(PypiRangeParameters);
