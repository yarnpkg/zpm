use bincode::{Decode, Encode};
use zpm_utils::{impl_file_string_from_str, impl_file_string_serialization, FromFileString, ToFileString, ToHumanString};

use crate::{extract::extract_version, range::RangeKind, Error, Range};

#[cfg(test)]
#[path = "./version.test.rs"]
mod version_tests;

#[derive(Clone, Debug, Decode, Encode, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum VersionRc {
    Number(u32),
    String(String),
}

#[derive(Clone, Debug, Default, Decode, Encode, PartialEq, Eq, Hash)]
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

    pub fn next_immediate_spec(&self) -> Version {
        if let Some(rc) = &self.rc {
            let mut all_but_last = rc[..rc.len() - 1]
                .to_vec();

            match rc.last() {
                Some(VersionRc::Number(n)) => {
                    all_but_last.push(VersionRc::Number(n + 1));

                    return Version {
                        major: self.major,
                        minor: self.minor,
                        patch: self.patch,
                        rc: Some(all_but_last),
                    };
                }

                Some(VersionRc::String(rc_str)) => {
                    let Some(last_char) = rc_str.chars().last() else {
                        panic!("VersionRc::String should always have a last character");
                    };

                    let mut all_but_last_str
                        = rc_str[..rc_str.len() - 1].to_string();

                    match last_char {
                        '-' => {
                            if rc_str.len() == 1 {
                                all_but_last_str.push('a');
                            } else {
                                all_but_last_str.push('0');
                            }
                        }

                        '0'..'9' => {
                            all_but_last_str.push((last_char as u8 + 1) as char);
                        }

                        '9' => {
                            all_but_last_str.push('a');
                        }

                        'a'..'z' => {
                            all_but_last_str.push((last_char as u8 + 1) as char);
                        }

                        'z' => {
                            all_but_last_str.push(last_char);
                            all_but_last_str.push('a');
                        }

                        _ => {
                            unreachable!("VersionRc::String should only contain alphanumeric characters and '-'");
                        }
                    }

                    all_but_last.push(VersionRc::String(all_but_last_str));

                    return Version {
                        major: self.major,
                        minor: self.minor,
                        patch: self.patch,
                        rc: Some(all_but_last),
                    };
                }

                None => {
                    // It shouldn't happen, but if it does the version doesn't have a rc so we can fall through
                }
            }
        }

        Version {
            major: self.major,
            minor: self.minor,
            patch: self.patch + 1,
            rc: Some(vec![VersionRc::Number(0)]),
        }
    }

    pub fn to_range(&self, kind: RangeKind) -> Range {
        match kind {
            RangeKind::Caret => Range::from_file_string(&format!("^{}", self.to_file_string())),
            RangeKind::Tilde => Range::from_file_string(&format!("~{}", self.to_file_string())),
            RangeKind::Exact => Range::from_file_string(&self.to_file_string()),
        }.expect("Converting a version to a range should be trivial")
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

impl_file_string_from_str!(Version);
impl_file_string_serialization!(Version);
