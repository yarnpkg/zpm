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
// Components above u32::MAX (https://github.com/yarnpkg/zpm/issues/319)
#[case("12.99999999204721.0", Version { major: 12, minor: 99999999204721, patch: 0, rc: None })]
#[case("0.99999999150214.0", Version { major: 0, minor: 99999999150214, patch: 0, rc: None })]
#[case("1.0.20240101123456", Version { major: 1, minor: 0, patch: 20240101123456, rc: None })]
#[case("4294967296.0.0", Version { major: 4294967296, minor: 0, patch: 0, rc: None })]
#[case("9007199254740991.9007199254740991.9007199254740991", Version { major: 9007199254740991, minor: 9007199254740991, patch: 9007199254740991, rc: None })]
#[case("1.0.0-rc.20240101123456", Version { major: 1, minor: 0, patch: 0, rc: Some(eco_vec![VersionRc::String(EcoString::from("rc")), VersionRc::Number(20240101123456)]) })]
// Numeric prerelease identifiers above MAX_SAFE_INTEGER are kept as strings, like node-semver
#[case("0.0.0-20231026123456789", Version { major: 0, minor: 0, patch: 0, rc: Some(eco_vec![VersionRc::String(EcoString::from("20231026123456789"))]) })]
fn test_version_parse(#[case] version: Version, #[case] expected: Version) {
    assert_eq!(version, expected);
}

#[rstest]
#[case("1.2.3", "1.2.4")]
#[case("1.2.3", "1.3.0")]
#[case("1.2.3", "2.0.0")]
#[case("1.2.3-rc.1", "1.2.3")]
#[case("4294967295.0.0", "4294967296.0.0")]
#[case("1.4294967295.0", "1.4294967296.0")]
#[case("1.0.20240101123456", "1.0.20240101123457")]
#[case("1.0.20240101123456", "1.1.0")]
#[case("1.0.0-rc.4294967296", "1.0.0-rc.10000000000")]
#[case("1.0.0-9007199254740991", "1.0.0-9007199254740992")]
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
    assert!(Version::from_file_string("1.2.9007199254740991").is_ok());
    assert!(Version::from_file_string("1.2.9007199254740992").is_err());
    assert!(Version::from_file_string("1.9007199254740992.0").is_err());
    assert!(Version::from_file_string("9007199254740992.0.0").is_err());
    assert!(Version::from_file_string("18446744073709551616.0.0").is_err());
}

#[test]
fn test_version_next_max_safe_integer() {
    let version
        = Version::from_file_string("9007199254740991.9007199254740991.9007199254740991-9007199254740991").unwrap();

    assert_eq!(version.next_major_rc(), Version { major: 9007199254740992, minor: 0, patch: 0, rc: Some(eco_vec![VersionRc::Number(0)]) });
    assert_eq!(version.next_minor_rc(), Version { major: 9007199254740991, minor: 9007199254740992, patch: 0, rc: Some(eco_vec![VersionRc::Number(0)]) });
    assert_eq!(version.next_patch_rc(), Version { major: 9007199254740991, minor: 9007199254740991, patch: 9007199254740992, rc: Some(eco_vec![VersionRc::Number(0)]) });
    assert_eq!(version.next_rc(), Version { major: 9007199254740991, minor: 9007199254740991, patch: 9007199254740991, rc: Some(eco_vec![VersionRc::Number(9007199254740992)]) });
    assert_eq!(version.next_immediate_spec(), Version { major: 9007199254740991, minor: 9007199254740991, patch: 9007199254740991, rc: Some(eco_vec![VersionRc::Number(9007199254740992)]) });
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
