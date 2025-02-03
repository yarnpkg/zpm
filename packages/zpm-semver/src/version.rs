use zpm_utils::{impl_serialization_traits, FromFileString, ToFileString, ToHumanString};

use crate::{extract::extract_version, Error};

#[cfg(test)]
#[path = "./version.test.rs"]
mod version_tests;

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "bincode", derive(bincode_derive::Decode, bincode_derive::Encode))]
pub enum VersionRc {
    Number(u32),
    String(String),
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "bincode", derive(bincode_derive::Decode, bincode_derive::Encode))]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
    pub rc: Option<Vec<VersionRc>>,
}

impl Version {
    pub fn new() -> Version {
        Version {
            major: 0,
            minor: 0,
            patch: 0,
            rc: None,
        }
    }

    pub fn new_from_components(major: u32, minor: u32, patch: u32, rc: Option<Vec<VersionRc>>) -> Version {
        Version {
            major,
            minor,
            patch,
            rc,
        }
    }

    pub fn next_major(&self) -> Version {
        Version {
            major: self.major + 1,
            minor: 0,
            patch: 0,
            rc: None,
        }
    }

    pub fn next_major_rc(&self) -> Version {
        Version {
            major: self.major + 1,
            minor: 0,
            patch: 0,
            rc: Some(vec![VersionRc::Number(0)]),
        }
    }

    pub fn next_minor(&self) -> Version {
        Version {
            major: self.major,
            minor: self.minor + 1,
            patch: 0,
            rc: None,
        }
    }

    pub fn next_minor_rc(&self) -> Version {
        Version {
            major: self.major,
            minor: self.minor + 1,
            patch: 0,
            rc: Some(vec![VersionRc::Number(0)]),
        }
    }

    pub fn next_patch(&self) -> Version {
        Version {
            major: self.major,
            minor: self.minor,
            patch: self.patch + 1,
            rc: None,
        }
    }

    pub fn next_patch_rc(&self) -> Version {
        Version {
            major: self.major,
            minor: self.minor,
            patch: self.patch + 1,
            rc: Some(vec![VersionRc::Number(0)]),
        }
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.major, self.minor, self.patch, self.rc.is_none(), &self.rc)
            .cmp(&(other.major, other.minor, other.patch, other.rc.is_none(), &other.rc))
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl FromFileString for Version {
    type Error = Error;

    fn from_file_string(src: &str) -> Result<Self, Error> {
        let mut iter = src.chars().peekable();

        let (version, _) = extract_version(&mut iter)
            .ok_or_else(|| Error::InvalidVersion(src.to_string()))?;

        if iter.peek().is_some() {
            return Err(Error::InvalidVersion(src.to_string()))
        }

        Ok(version)
    }
}

impl ToFileString for Version {
    fn to_file_string(&self) -> String {
        let mut res = format!("{}.{}.{}", self.major, self.minor, self.patch);

        if let Some(rc) = &self.rc {
            res.push('-');

            for segment in rc.iter() {
                match segment {
                    VersionRc::Number(n) => {
                        res.push_str(&n.to_string());
                        res.push('.');
                    }

                    VersionRc::String(s) => {
                        res.push_str(s);
                        res.push('.');
                    }
                }
            }

            res.pop();
        }

        res
    }
}

impl ToHumanString for Version {
    fn to_print_string(&self) -> String {
        self.to_file_string()
    }
}

impl_serialization_traits!(Version);
