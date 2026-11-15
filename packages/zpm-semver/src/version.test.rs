use rstest::rstest;
use zpm_utils::FromFileString;

use crate::{Version, VersionRc};
use zpm_utils::{eco_vec, EcoString};

#[rstest]
#[case("1.2.3", Version { major: 1, minor: 2, patch: 3, rc: None })]
#[case("1.2.3-rc", Version { major: 1, minor: 2, patch: 3, rc: Some(eco_vec![VersionRc::String(EcoString::from("rc"))]) })]
#[case("1.2.3-rc.1", Version { major: 1, minor: 2, patch: 3, rc: Some(eco_vec![VersionRc::String(EcoString::from("rc")), VersionRc::Number(1)]) })]
#[case("1.2.3-rc.1.32a", Version { major: 1, minor: 2, patch: 3, rc: Some(eco_vec![VersionRc::String(EcoString::from("rc")), VersionRc::Number(1), VersionRc::String(EcoString::from("32a"))]) })]
#[case("5.11.0-next.1603014861.18546659943e6c5744ce67403b1c78c1993ccf84", Version { major: 5, minor: 11, patch: 0, rc: Some(eco_vec![VersionRc::String(EcoString::from("next")), VersionRc::Number(1603014861), VersionRc::String(EcoString::from("18546659943e6c5744ce67403b1c78c1993ccf84"))]) })]
fn test_version_parse(#[case] version: Version, #[case] expected: Version) {
    assert_eq!(version, expected);
}

#[rstest]
#[case("1.2.3", "1.2.4")]
#[case("1.2.3", "1.3.0")]
#[case("1.2.3", "2.0.0")]
#[case("1.2.3-rc.1", "1.2.3")]
fn test_version_lt(#[case] left: Version, #[case] right: Version) {
    assert!(left < right);
}

#[rstest]
#[case("1.2.0", "1.2.1-0")]
#[case("1.2.9", "1.2.10-0")]
#[case("1.2.0-42", "1.2.0-43")]
#[case("1.2.0-rc.1", "1.2.0-rc.2")]
#[case("1.2.0-rc", "1.2.0-rd")]
#[case("1.0.0-x-y-z.--", "1.0.0-x-y-z.-0")]
#[case("1.0.0-x-y-z.-", "1.0.0-x-y-z.a")]
fn test_version_next_immediate(#[case] left: Version, #[case] right: Version) {
    assert_eq!(left.next_immediate_spec(), right);
}

#[test]
fn test_version_max_length() {
    let long_prerelease = "a".repeat(257);
    let version = format!("1.2.3-{}", long_prerelease);
    assert!(Version::from_file_string(&version).is_err());
}

#[test]
fn test_version_max_safe_integer() {
    assert!(Version::from_file_string("1.2.9007199254740992").is_err());
}

#[test]
fn test_version_max_safe_component_length() {
    // From node-semver coerce tests: `'1'.repeat(17)` is rejected.
    let version = "1".repeat(17);
    assert!(Version::from_file_string(&version).is_err());
}

#[rstest]
#[case("1.2.3")]
#[case("0.0.0")]
#[case("v1.2.3")]
#[case("10.20.30")]
#[case("1.2.3-0")]
#[case("1.2.3-rc.1")]
#[case("1.2.3-0a.01a.-")]
#[case("1.2.3-alpha-1.beta--2")]
#[case("1.2.3+build")]
#[case("1.2.3+001.sha-5114f85")]
#[case("1.2.3-rc.1+build.2")]
#[case(" 1.2.3")]
#[case("1.2.3 ")]
fn test_version_from_strict_str_valid(#[case] version: &str) {
    assert_eq!(Version::from_strict_str(version), Some(Version::from_file_string(version.trim()).unwrap()));
}

// Each of these is rejected by node-semver's strict mode (`new SemVer(...)`)
#[rstest]
#[case("")]
#[case("1")]
#[case("1.0")]
#[case("1.x")]
#[case("1.2.x")]
#[case("*")]
#[case("x")]
#[case("01.2.3")]
#[case("1.02.3")]
#[case("1.2.03")]
#[case("1.2.3-01")]
#[case("1.2.3.4")]
#[case("1.2.3-")]
#[case("1.2.3-rc.")]
#[case("1.2.3-rc..1")]
#[case("1.2.3+")]
#[case("1.2.3_beta")]
#[case("1.2.3-béta")]
#[case("=1.2.3")]
#[case("V1.2.3")]
#[case("1.2.3-rc 1")]
#[case(">=1.2.3")]
#[case("1.2.9007199254740992")]
#[case("latest")]
fn test_version_from_strict_str_invalid(#[case] version: &str) {
    assert_eq!(Version::from_strict_str(version), None);
}
