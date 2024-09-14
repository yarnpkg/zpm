use bincode::{Decode, Encode};

use crate::{error::Error, yarn_serialization_protocol};

use super::extract;

#[cfg(test)]
#[path = "./version.test.rs"]
mod version_tests;

#[derive(Clone, Debug, Decode, Encode, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum VersionRc {
    Number(u32),
    String(String),
}

#[derive(Clone, Debug, Decode, Default, Encode, PartialEq, Eq, Hash)]
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

yarn_serialization_protocol!(Version, "", {
    deserialize(src) {
        let mut iter = src.chars().peekable();

        let (version, _) = extract::extract_version(&mut iter)
            .ok_or_else(|| Error::InvalidSemverVersion(src.to_string()))?;

        if iter.peek().is_some() {
            return Err(Error::InvalidSemverVersion(src.to_string()))
        }

        Ok(version)
    }

    serialize(&self) {
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
});
