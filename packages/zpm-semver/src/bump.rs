use zpm_utils::{EcoVec, FromFileString, ToFileString};

use crate::{
    Error,
    Version,
    VersionRc,
};

#[cfg(test)]
#[path = "./bump.test.rs"]
mod bump_tests;

/// The incremental strategies supported by `yarn version`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BumpStrategy {
    Major,
    Minor,
    Patch,
    Premajor,
    Preminor,
    Prepatch,
    Prerelease,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum PrereleasePatternComponent {
    Literal(String),
    Counter,
}

/// A prerelease pattern such as `rc.%n`, where each `%n` component is a
/// counter that gets incremented when the current version matches the
/// pattern, and reset to `1` otherwise.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PrereleasePattern {
    components: Vec<PrereleasePatternComponent>,
}

impl Default for PrereleasePattern {
    fn default() -> Self {
        PrereleasePattern {
            components: vec![
                PrereleasePatternComponent::Literal("rc".to_string()),
                PrereleasePatternComponent::Counter,
            ],
        }
    }
}

impl FromFileString for PrereleasePattern {
    type Error = Error;

    fn from_file_string(src: &str) -> Result<Self, Error> {
        let components = src.split('.').map(|component| {
            if component == "%n" {
                return Ok(PrereleasePatternComponent::Counter);
            }

            let is_identifier
                = !component.is_empty() && component.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
            let has_leading_zero
                = component.len() > 1 && component.starts_with('0') && component.chars().all(|c| c.is_ascii_digit());

            if !is_identifier || has_leading_zero {
                return Err(Error::InvalidPrereleasePattern(src.to_string()));
            }

            Ok(PrereleasePatternComponent::Literal(component.to_string()))
        }).collect::<Result<Vec<_>, _>>()?;

        Ok(PrereleasePattern {components})
    }
}

impl ToFileString for PrereleasePattern {
    fn to_file_string(&self) -> String {
        self.components.iter()
            .map(|component| match component {
                PrereleasePatternComponent::Literal(literal) => literal.as_str(),
                PrereleasePatternComponent::Counter => "%n",
            })
            .collect::<Vec<_>>()
            .join(".")
    }
}

impl PrereleasePattern {
    /// Returns `base` (minus its prerelease identifiers) with prerelease
    /// identifiers generated from the pattern. The counters are incremented
    /// if `current` is a prerelease of the same version and its identifiers
    /// match the pattern; otherwise they are reset to `1`.
    pub fn apply(&self, base: &Version, current: &Version) -> Version {
        enum Part<'a> {
            Literal(&'a str),
            Counter {increment: u32},
            Reset,
        }

        let stable
            = base.to_stable();

        // If the version we have in mind has nothing in common with the
        // current one we don't want to reuse its prerelease identifiers
        // (1.0.0-rc.5 -> 1.1.0-rc.1), so we start in the unmatched state.
        let mut pattern_matched
            = current.to_stable() == stable;

        let current_parts: &[VersionRc]
            = current.rc.as_ref().map_or(&[], |rc| rc.as_slice());

        let mut current_index
            = 0;

        let mut parts
            = Vec::with_capacity(self.components.len());

        for component in &self.components {
            match component {
                PrereleasePatternComponent::Literal(literal) => {
                    parts.push(Part::Literal(literal.as_str()));

                    let is_same = match current_parts.get(current_index) {
                        Some(VersionRc::String(value)) => value.as_str() == literal,
                        Some(VersionRc::Number(value)) => value.to_string() == *literal,
                        None => false,
                    };

                    if is_same {
                        current_index += 1;
                    } else {
                        pattern_matched = false;
                    }
                },

                PrereleasePatternComponent::Counter => {
                    let current_number = match current_parts.get(current_index) {
                        Some(VersionRc::Number(value)) => {
                            current_index += 1;
                            Some(*value)
                        },

                        _ => None,
                    };

                    match current_number {
                        Some(value) if pattern_matched => {
                            parts.push(Part::Counter {increment: value + 1});
                        },

                        _ => {
                            parts.push(Part::Reset);
                            pattern_matched = false;
                        },
                    }
                },
            }
        }

        // If there are more prerelease identifiers than the pattern needs,
        // it's also a mismatch
        pattern_matched &= current_index == current_parts.len();

        let rc = parts.into_iter()
            .map(|part| match part {
                Part::Literal(literal) => match literal.parse() {
                    Ok(number) => VersionRc::Number(number),
                    Err(_) => VersionRc::String(literal.into()),
                },

                Part::Counter {increment} if pattern_matched
                    => VersionRc::Number(increment),

                Part::Counter {..} | Part::Reset
                    => VersionRc::Number(1),
            })
            .collect::<Vec<_>>();

        Version {
            rc: Some(EcoVec::from(rc)),
            ..stable
        }
    }
}

impl Version {
    /// Returns the version without its prerelease identifiers.
    pub fn to_stable(&self) -> Version {
        Version {
            major: self.major,
            minor: self.minor,
            patch: self.patch,
            rc: None,
        }
    }

    /// Same as node-semver's `inc(version, "major")`: `1.2.3` and `2.0.0-rc.1`
    /// both become `2.0.0`.
    pub fn inc_major(&self) -> Version {
        let major = match self.minor != 0 || self.patch != 0 || self.rc.is_none() {
            true => self.major + 1,
            false => self.major,
        };

        Version::new_from_components(major, 0, 0, None)
    }

    /// Same as node-semver's `inc(version, "minor")`: `1.2.3` and `1.3.0-rc.1`
    /// both become `1.3.0`.
    pub fn inc_minor(&self) -> Version {
        let minor = match self.patch != 0 || self.rc.is_none() {
            true => self.minor + 1,
            false => self.minor,
        };

        Version::new_from_components(self.major, minor, 0, None)
    }

    /// Same as node-semver's `inc(version, "patch")`: `1.2.3` and `1.2.4-rc.1`
    /// both become `1.2.4`.
    pub fn inc_patch(&self) -> Version {
        let patch = match self.rc.is_none() {
            true => self.patch + 1,
            false => self.patch,
        };

        Version::new_from_components(self.major, self.minor, patch, None)
    }

    /// Same as node-semver's `inc(version, "prerelease")` (without
    /// identifier): increments the last numeric prerelease identifier
    /// (appending `0` if there's none), or bumps a stable version to the
    /// first prerelease of the next patch.
    pub fn inc_prerelease(&self) -> Version {
        let Some(rc) = &self.rc else {
            return Version {
                rc: Some(EcoVec::from([VersionRc::Number(0)])),
                ..self.inc_patch()
            };
        };

        let mut rc
            = rc.clone();

        let last_number = rc.make_mut().iter_mut().rev().find_map(|part| match part {
            VersionRc::Number(value) => Some(value),
            VersionRc::String(_) => None,
        });

        match last_number {
            Some(value) => *value += 1,
            None => rc.push(VersionRc::Number(0)),
        }

        Version {
            rc: Some(rc),
            ..self.clone()
        }
    }

    /// Computes the version `yarn version <strategy>` bumps to.
    ///
    /// The `major`, `minor`, and `patch` strategies follow node-semver, so a
    /// prerelease bumps to its own stable version if it's of the right level
    /// (`1.2.0-3` + `minor` = `1.2.0`).
    ///
    /// The `premajor`, `preminor`, and `prepatch` strategies only increment
    /// the prerelease identifiers if the current version is already a
    /// prerelease of the requested level or higher (`1.2.0-3` + `prepatch`
    /// = `1.2.0-4`); otherwise they add prerelease identifiers to the next
    /// version of the requested level (`1.2.3-3` + `preminor` = `1.3.0-0`).
    /// The `prerelease` strategy is equivalent to `prepatch`.
    ///
    /// When a prerelease pattern is given it's used to generate the new
    /// prerelease identifiers, including for the `major`, `minor`, and
    /// `patch` strategies.
    pub fn bump(&self, strategy: BumpStrategy, prerelease: Option<&PrereleasePattern>) -> Version {
        let (base, is_prerelease_strategy) = match strategy {
            BumpStrategy::Major => (self.inc_major(), false),
            BumpStrategy::Minor => (self.inc_minor(), false),
            BumpStrategy::Patch => (self.inc_patch(), false),
            BumpStrategy::Premajor => (self.inc_major(), true),
            BumpStrategy::Preminor => (self.inc_minor(), true),
            BumpStrategy::Prepatch | BumpStrategy::Prerelease => (self.inc_patch(), true),
        };

        if let Some(prerelease) = prerelease {
            return prerelease.apply(&base, self);
        }

        if !is_prerelease_strategy {
            return base;
        }

        // This implies that the current version is already a prerelease of
        // the requested level, so we can just bump its identifiers
        if base == self.to_stable() {
            return self.inc_prerelease();
        }

        Version {
            rc: Some(EcoVec::from([VersionRc::Number(0)])),
            ..base
        }
    }

    /// Parses a version the way node-semver's `valid` does in strict mode:
    /// all three components are required, numeric identifiers can't have
    /// leading zeros, and only a single leading `v` is accepted. Build
    /// metadata is accepted but discarded.
    pub fn from_strict_str(src: &str) -> Result<Version, Error> {
        let version
            = Version::from_file_string(src)?;

        let without_prefix
            = src.strip_prefix('v').unwrap_or(src);
        let without_build
            = without_prefix.split_once('+').map_or(without_prefix, |(main, _)| main);

        if !src.is_ascii() || version.to_file_string() != without_build {
            return Err(Error::InvalidVersion(src.to_string()));
        }

        Ok(version)
    }
}
