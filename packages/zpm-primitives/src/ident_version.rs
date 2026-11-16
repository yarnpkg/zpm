use colored::Colorize;
use zpm_utils::{impl_file_string_from_str, impl_file_string_serialization, DataType, FromFileString, ToFileString, ToHumanString};

use crate::IdentError;

use super::{Ident, split_ident_and_selector};

#[derive(thiserror::Error, Clone, Debug)]
pub enum IdentVersionError {
    #[error("Invalid package version ({0}); expected <name>@<version>")]
    SyntaxError(String),

    #[error(transparent)]
    IdentError(#[from] IdentError),

    #[error("The range {} must be a valid semver version", DataType::Range.colorize(.0))]
    InvalidVersion(String),
}

/// A package name along with one exact version of this package, as written
/// `name@1.2.3` or `@scope/name@1.2.3`.
///
/// Unlike descriptors, the version must be a complete semver version as
/// accepted by node-semver's strict mode: ranges (including `=1.2.3`), partial
/// versions (`1`, `1.2`, `1.x`), wildcards and tags are all rejected. Build
/// metadata is accepted but not kept, since `Version` doesn't store it.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct IdentVersion {
    pub ident: Ident,
    pub version: zpm_semver::Version,
}

impl IdentVersion {
    pub fn new(ident: Ident, version: zpm_semver::Version) -> IdentVersion {
        IdentVersion {
            ident,
            version,
        }
    }
}

impl FromFileString for IdentVersion {
    type Error = IdentVersionError;

    fn from_file_string(src: &str) -> Result<Self, Self::Error> {
        let (ident_str, version_str) = split_ident_and_selector(src);
        let version_str = version_str
            .filter(|version_str| !version_str.is_empty())
            .ok_or_else(|| IdentVersionError::SyntaxError(src.to_string()))?;

        let ident
            = Ident::from_file_string(ident_str)?;

        let version
            = zpm_semver::Version::from_strict_str(version_str)
                .ok_or_else(|| IdentVersionError::InvalidVersion(version_str.to_string()))?;

        Ok(IdentVersion::new(ident, version))
    }
}

impl ToFileString for IdentVersion {
    fn to_file_string(&self) -> String {
        format!("{}@{}", self.ident.to_file_string(), self.version.to_file_string())
    }
}

impl ToHumanString for IdentVersion {
    fn to_print_string(&self) -> String {
        format!("{}{}{}",
            self.ident.to_print_string(),
            "@".truecolor(0, 175, 175),
            self.version.to_print_string()
        )
    }
}

impl_file_string_from_str!(IdentVersion);
impl_file_string_serialization!(IdentVersion);

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use zpm_utils::{FromFileString, ToFileString};

    use super::{IdentVersion, IdentVersionError};

    #[rstest]
    #[case("foo@1.2.3", "foo@1.2.3")]
    #[case("@scope/foo@1.2.3", "@scope/foo@1.2.3")]
    #[case("foo@0.0.0", "foo@0.0.0")]
    #[case("foo@1.2.3-rc.1", "foo@1.2.3-rc.1")]
    #[case("@scope/foo@1.2.3-alpha-1.beta--2", "@scope/foo@1.2.3-alpha-1.beta--2")]
    #[case("foo@1.2.3+build.1", "foo@1.2.3")]
    #[case("foo@1.2.3-rc.1+build.1", "foo@1.2.3-rc.1")]
    #[case("foo@v1.2.3", "foo@1.2.3")]
    fn test_ident_version_valid(#[case] src: &str, #[case] expected: &str) {
        let ident_version
            = IdentVersion::from_file_string(src).unwrap();

        assert_eq!(ident_version.to_file_string(), expected);
        assert_eq!(IdentVersion::from_file_string(expected).unwrap(), ident_version);
    }

    #[test]
    fn test_ident_version_components() {
        let ident_version
            = IdentVersion::from_file_string("@scope/foo@1.2.3").unwrap();

        assert_eq!(ident_version.ident.as_str(), "@scope/foo");
        assert_eq!(ident_version.version.to_file_string(), "1.2.3");
    }

    #[rstest]
    #[case("foo@1")]
    #[case("foo@1.0")]
    #[case("foo@1.x")]
    #[case("foo@1.0.x")]
    #[case("foo@*")]
    #[case("foo@x")]
    #[case("foo@^1.0.0")]
    #[case("foo@~1.0.0")]
    #[case("foo@>=1.0.0")]
    #[case("foo@=1.0.0")]
    #[case("foo@=1")]
    #[case("foo@1.0.0 || 2.0.0")]
    #[case("foo@01.0.0")]
    #[case("foo@V1.0.0")]
    #[case("foo@latest")]
    #[case("foo@npm:1.0.0")]
    #[case("@scope/foo@^1.0.0")]
    fn test_ident_version_invalid_version(#[case] src: &str) {
        assert!(matches!(IdentVersion::from_file_string(src), Err(IdentVersionError::InvalidVersion(_))));
    }

    #[rstest]
    #[case("foo")]
    #[case("foo@")]
    #[case("@scope/foo")]
    #[case("@scope/foo@")]
    fn test_ident_version_missing_version(#[case] src: &str) {
        assert!(matches!(IdentVersion::from_file_string(src), Err(IdentVersionError::SyntaxError(_))));
    }

    #[rstest]
    #[case("@scope@1.0.0")]
    #[case("foo/bar@1.0.0")]
    fn test_ident_version_invalid_ident(#[case] src: &str) {
        assert!(matches!(IdentVersion::from_file_string(src), Err(IdentVersionError::IdentError(_))));
    }
}
