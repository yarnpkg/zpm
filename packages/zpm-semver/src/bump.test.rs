use rstest::rstest;
use zpm_utils::{FromFileString, ToFileString};

use crate::{BumpStrategy, PrereleasePattern, Version};

#[rstest]
// Stable versions (same as node-semver)
#[case("1.2.3", BumpStrategy::Major, "2.0.0")]
#[case("1.2.3", BumpStrategy::Minor, "1.3.0")]
#[case("1.2.3", BumpStrategy::Patch, "1.2.4")]
#[case("1.2.3", BumpStrategy::Premajor, "2.0.0-0")]
#[case("1.2.3", BumpStrategy::Preminor, "1.3.0-0")]
#[case("1.2.3", BumpStrategy::Prepatch, "1.2.4-0")]
#[case("1.2.3", BumpStrategy::Prerelease, "1.2.4-0")]
// Stable strategies on prereleases (same as node-semver): a prerelease of the
// right level is bumped to its own stable version
#[case("1.2.3-rc.1", BumpStrategy::Patch, "1.2.3")]
#[case("1.2.3-rc.1", BumpStrategy::Minor, "1.3.0")]
#[case("1.2.3-rc.1", BumpStrategy::Major, "2.0.0")]
#[case("1.2.0-3", BumpStrategy::Patch, "1.2.0")]
#[case("1.2.0-3", BumpStrategy::Minor, "1.2.0")]
#[case("1.2.0-3", BumpStrategy::Major, "2.0.0")]
#[case("1.0.0-3", BumpStrategy::Minor, "1.0.0")]
#[case("1.0.0-3", BumpStrategy::Major, "1.0.0")]
#[case("2.0.0-rc.1", BumpStrategy::Major, "2.0.0")]
// Prerelease strategies on a prerelease of a patch
#[case("1.2.3-9", BumpStrategy::Prepatch, "1.2.3-10")]
#[case("1.2.3-9", BumpStrategy::Preminor, "1.3.0-0")]
#[case("1.2.3-9", BumpStrategy::Premajor, "2.0.0-0")]
#[case("1.2.3-9", BumpStrategy::Prerelease, "1.2.3-10")]
// Prerelease strategies on a prerelease of a minor (node-semver would return
// 1.2.1-0 for prepatch)
#[case("1.2.0-3", BumpStrategy::Prepatch, "1.2.0-4")]
#[case("1.2.0-9", BumpStrategy::Prepatch, "1.2.0-10")]
#[case("1.2.0-9", BumpStrategy::Preminor, "1.2.0-10")]
#[case("1.2.0-9", BumpStrategy::Premajor, "2.0.0-0")]
// Prerelease strategies on a prerelease of a major (node-semver would return
// 1.0.1-0 / 1.1.0-0 / 2.0.0-0)
#[case("1.0.0-9", BumpStrategy::Prepatch, "1.0.0-10")]
#[case("1.0.0-9", BumpStrategy::Preminor, "1.0.0-10")]
#[case("1.0.0-9", BumpStrategy::Premajor, "1.0.0-10")]
// Prerelease identifiers (same as node-semver's `prerelease`)
#[case("1.2.3-0", BumpStrategy::Prerelease, "1.2.3-1")]
#[case("1.2.3-alpha.0", BumpStrategy::Prerelease, "1.2.3-alpha.1")]
#[case("1.2.3-alpha.9.beta", BumpStrategy::Prerelease, "1.2.3-alpha.10.beta")]
#[case("1.2.3-alpha.10.0.beta", BumpStrategy::Prerelease, "1.2.3-alpha.10.1.beta")]
#[case("1.2.3-a.1.b.2.c", BumpStrategy::Prerelease, "1.2.3-a.1.b.3.c")]
#[case("1.2.3-alpha", BumpStrategy::Prerelease, "1.2.3-alpha.0")]
fn test_bump(#[case] version: Version, #[case] strategy: BumpStrategy, #[case] expected: Version) {
    assert_eq!(version.bump(strategy, None).to_file_string(), expected.to_file_string());
}

#[rstest]
#[case("1.2.3", BumpStrategy::Prerelease, "rc.%n", "1.2.4-rc.1")]
#[case("1.2.3-rc.41", BumpStrategy::Prerelease, "rc.%n", "1.2.3-rc.42")]
#[case("1.2.3-rc.41", BumpStrategy::Preminor, "rc.%n", "1.3.0-rc.1")]
#[case("1.2.3-a.1.b.2.c", BumpStrategy::Prerelease, "a.%n.b.%n.c", "1.2.3-a.2.b.3.c")]
#[case("1.2.3-alpha.999", BumpStrategy::Prerelease, "beta.%n", "1.2.3-beta.1")]
#[case("1.2.3-alpha.1", BumpStrategy::Prerelease, "beta.%n", "1.2.3-beta.1")]
// Partial match: all counters are reset
#[case("1.2.3-a.1.b.2.c", BumpStrategy::Prerelease, "a.%n.b.c.%n", "1.2.3-a.1.b.c.1")]
// Extra identifiers: all counters are reset
#[case("1.2.3-rc.1.2", BumpStrategy::Prerelease, "rc.%n", "1.2.3-rc.1")]
// Numeric literals are matched against numeric identifiers
#[case("1.2.3-1.4", BumpStrategy::Prerelease, "1.%n", "1.2.3-1.5")]
// Stable strategies also honour the pattern
#[case("1.0.32", BumpStrategy::Patch, "alpha.0", "1.0.33-alpha.0")]
#[case("1.0.32", BumpStrategy::Minor, "alpha.0", "1.1.0-alpha.0")]
#[case("1.0.32", BumpStrategy::Major, "rc.%n", "2.0.0-rc.1")]
#[case("1.0.33-rc.1", BumpStrategy::Patch, "rc.%n", "1.0.33-rc.2")]
fn test_bump_with_prerelease(#[case] version: Version, #[case] strategy: BumpStrategy, #[case] pattern: String, #[case] expected: Version) {
    let pattern
        = PrereleasePattern::from_file_string(&pattern).unwrap();

    assert_eq!(version.bump(strategy, Some(&pattern)).to_file_string(), expected.to_file_string());
}

#[rstest]
#[case("1.2.3", "rc.%n", "1.2.3-rc.1")]
#[case("1.2.3-rc.41", "rc.%n", "1.2.3-rc.42")]
fn test_prerelease_pattern_apply_same_version(#[case] version: Version, #[case] pattern: String, #[case] expected: Version) {
    let pattern
        = PrereleasePattern::from_file_string(&pattern).unwrap();

    assert_eq!(pattern.apply(&version.to_stable(), &version).to_file_string(), expected.to_file_string());
}

#[test]
fn test_prerelease_pattern_default() {
    assert_eq!(PrereleasePattern::default(), PrereleasePattern::from_file_string("rc.%n").unwrap());
}

#[rstest]
#[case("")]
#[case("rc.")]
#[case("rc..%n")]
#[case("rc.01")]
#[case("rc_1")]
#[case("rc.%m")]
fn test_prerelease_pattern_invalid(#[case] pattern: &str) {
    assert!(PrereleasePattern::from_file_string(pattern).is_err());
}

#[rstest]
#[case("1.2.3", "1.2.3")]
#[case("v1.2.3", "1.2.3")]
#[case("1.2.3-rc.1", "1.2.3-rc.1")]
#[case("1.2.3+build.5", "1.2.3")]
#[case("1.2.3-rc.1+build", "1.2.3-rc.1")]
fn test_from_strict_str_valid(#[case] src: &str, #[case] expected: &str) {
    assert_eq!(Version::from_strict_str(src).unwrap().to_file_string(), expected);
}

#[rstest]
#[case("1.2")]
#[case("1.2.x")]
#[case("01.2.3")]
#[case("1.2.3-01")]
#[case("1.2.3-")]
#[case("vv1.2.3")]
#[case("invalid")]
fn test_from_strict_str_invalid(#[case] src: &str) {
    assert!(Version::from_strict_str(src).is_none());
}
