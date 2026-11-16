use rkyv::Archive;
use zpm_utils::{impl_file_string_from_str, impl_file_string_serialization, DataType, FromFileString, ToFileString, ToHumanString};
use zpm_utils::{EcoString, EcoVec};

use crate::{
    extract::extract_version,
    range::RangeKind,
    Error,
    Range,
    MAX_LENGTH,
};

#[cfg(test)]
#[path = "./version.test.rs"]
mod version_tests;

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Archive, rkyv::Serialize, rkyv::Deserialize)]
#[rkyv(derive(PartialEq, Eq, Hash, PartialOrd, Ord))]
pub enum VersionRc {
    Number(u32),
    String(EcoString),
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Archive, rkyv::Serialize, rkyv::Deserialize)]
#[rkyv(derive(PartialEq, Eq, Hash, PartialOrd, Ord))]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
    pub rc: Option<EcoVec<VersionRc>>,
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

    pub fn new_from_components(major: u32, minor: u32, patch: u32, rc: Option<EcoVec<VersionRc>>) -> Version {
        Version {
            major,
            minor,
            patch,
            rc,
        }
    }

    /// Parses a version using the same grammar as node-semver's strict mode
    /// (`new SemVer(version)`), which is what Yarn 4 uses to filter the
    /// versions listed by a registry. Unlike `from_file_string`, it rejects
    /// the loose forms (`1.0`, `1.x`, `01.2.3`, `1.2.3-01`, ...) that the
    /// general parser coerces into a version. Like node-semver, it ignores
    /// surrounding whitespace.
    pub fn from_strict_str(src: &str) -> Option<Version> {
        let src
            = src.trim();

        if !is_strict_version(src) {
            return None;
        }

        Version::from_file_string(src).ok()
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
            rc: Some(EcoVec::from([VersionRc::Number(0)])),
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
            rc: Some(EcoVec::from([VersionRc::Number(0)])),
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
            rc: Some(EcoVec::from([VersionRc::Number(0)])),
        }
    }

    pub fn next_rc(&self) -> Version {
        let mut next
            = self.clone();

        if let Some(rc) = next.rc.as_mut() {
            let incremented = match rc.make_mut().last_mut() {
                Some(VersionRc::Number(n)) => {
                    *n += 1;
                    true
                }
                _ => false,
            };
            if !incremented {
                rc.push(VersionRc::Number(0));
            }
        } else {
            next.patch += 1;
            next.rc = Some(EcoVec::from([VersionRc::Number(0)]));
        }

        next
    }

    pub fn next_immediate_spec(&self) -> Version {
        if let Some(rc) = &self.rc {
            let mut all_but_last: EcoVec<VersionRc>
                = rc[..rc.len() - 1].into();

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
                    let rc_str = rc_str.as_str();
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

                    all_but_last.push(VersionRc::String(all_but_last_str.into()));

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
            rc: Some(EcoVec::from([VersionRc::Number(0)])),
        }
    }

    pub fn to_range(&self, kind: RangeKind) -> Range {
        match kind {
            RangeKind::Caret => Range::from_file_string(&format!("^{}", self.to_file_string())),
            RangeKind::Tilde => Range::from_file_string(&format!("~{}", self.to_file_string())),
            RangeKind::Exact => Range::from_file_string(&self.to_file_string()),
        }.expect("Converting a version to a range should be trivial")
    }

    pub fn to_rc_string(&self) -> Option<String> {
        let Some(rc) = &self.rc else {
            return None;
        };

        let mut res
            = String::new();

        for segment in rc {
            match segment {
                VersionRc::Number(n) => {
                    res.push_str(&n.to_string());
                }
                VersionRc::String(s) => {
                    res.push_str(s.as_str());
                }
            }

            res.push('.');
        }

        res.pop();

        Some(res)
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

/// Checks a version against node-semver's strict grammar:
/// `v?N.N.N(-ID(.ID)*)?(+BUILD(.BUILD)*)?`, where numbers (including numeric
/// prerelease identifiers) can't have leading zeros.
fn is_strict_version(src: &str) -> bool {
    let bytes
        = src.as_bytes();

    let mut index
        = usize::from(bytes.first() == Some(&b'v'));

    // Consumes the longest run of bytes matching `predicate` and returns it
    let consume = |index: &mut usize, predicate: fn(&u8) -> bool| -> Option<&[u8]> {
        let start
            = *index;

        while bytes.get(*index).is_some_and(predicate) {
            *index += 1;
        }

        (*index > start).then(|| &bytes[start..*index])
    };

    let identifier
        = |index: &mut usize| consume(index, |c| c.is_ascii_alphanumeric() || *c == b'-');

    let is_number
        = |id: &[u8]| id.iter().all(u8::is_ascii_digit);
    let has_leading_zero
        = |id: &[u8]| id.len() > 1 && id[0] == b'0';

    for component in 0..3 {
        if component > 0 {
            if bytes.get(index) != Some(&b'.') {
                return false;
            }

            index += 1;
        }

        match consume(&mut index, u8::is_ascii_digit) {
            Some(id) if !has_leading_zero(id)
                => {},

            _
                => return false,
        }
    }

    if bytes.get(index) == Some(&b'-') {
        loop {
            index += 1;

            match identifier(&mut index) {
                Some(id) if !is_number(id) || !has_leading_zero(id)
                    => {},

                _
                    => return false,
            }

            if bytes.get(index) != Some(&b'.') {
                break;
            }
        }
    }

    if bytes.get(index) == Some(&b'+') {
        loop {
            index += 1;

            if identifier(&mut index).is_none() {
                return false;
            }

            if bytes.get(index) != Some(&b'.') {
                break;
            }
        }
    }

    index == bytes.len()
}

impl FromFileString for Version {
    type Error = Error;

    fn from_file_string(src: &str) -> Result<Self, Error> {
        if src.len() > MAX_LENGTH {
            return Err(Error::InvalidVersion(src.to_string()));
        }

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
                        res.push_str(s.as_str());
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
        DataType::Reference.colorize(&self.to_file_string())
    }
}

impl_file_string_from_str!(Version);
impl_file_string_serialization!(Version);
