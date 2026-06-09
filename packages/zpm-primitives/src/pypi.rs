use std::{cmp::Ordering, str::FromStr};

use rkyv::Archive;
use zpm_semver::VersionRc;
use zpm_utils::{DataType, EcoVec, FromFileString, ToFileString, ToHumanString, impl_file_string_from_str, impl_file_string_serialization};

#[derive(thiserror::Error, Clone, Debug, PartialEq, Eq)]
pub enum PypiError {
    #[error("Invalid PEP 440 version: {0}")]
    InvalidVersion(String),

    #[error("Invalid PEP 440 specifier set: {0}")]
    InvalidSpecifier(String),

    #[error("Cannot project PEP 440 version to semver: {0}")]
    InvalidSemverProjection(String),
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

    pub fn intersection(&self, other: &Self) -> Result<Self, PypiError> {
        if self.is_any() {
            return Ok(other.clone());
        }

        if other.is_any() {
            return Ok(self.clone());
        }

        Self::from_file_string(&format!("{},{}", self.raw, other.raw))
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

pub fn canonicalize_pypi_name(name: &str) -> String {
    let mut result
        = String::new();
    let mut previous_was_separator
        = false;

    for ch in name.chars().flat_map(char::to_lowercase) {
        if matches!(ch, '-' | '_' | '.') {
            if !previous_was_separator {
                result.push('-');
                previous_was_separator = true;
            }
        } else {
            result.push(ch);
            previous_was_separator = false;
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use zpm_utils::ToFileString;

    use super::*;

    #[test]
    fn test_canonicalize_pypi_name() {
        assert_eq!("friendly-bard", canonicalize_pypi_name("Friendly__Bard"));
        assert_eq!("a-b-c", canonicalize_pypi_name("A-_-.B___C"));
    }

    #[test]
    fn test_pypi_specifier_intersection() {
        let a
            = PypiSpecifierSet::from_file_string(">=1.0.0").unwrap();
        let b
            = PypiSpecifierSet::from_file_string("<2.0.0").unwrap();

        assert_eq!(">=1.0.0, <2.0.0", a.intersection(&b).unwrap().to_file_string());
    }
}
