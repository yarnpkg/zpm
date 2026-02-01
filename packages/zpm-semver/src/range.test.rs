use rstest::rstest;
use zpm_utils::FromFileString;

use crate::{range::{OperatorType, Token, TokenType}, Range, Version};

#[rstest]
#[case("1.2.3", "1.2.3", true)]

#[case("^1.2.3", "1.2.0", false)]
#[case("^1.2.3", "1.2.3", true)]
#[case("^1.2.3", "1.2.10", true)]
#[case("^1.2.3", "1.10.0", true)]
#[case("^1.2.3", "1.10.0-rc", false)]
#[case("^1.2.3", "2.0.0-rc", false)]
#[case("^1.2.3", "2.0.0-0", false)]
#[case("^1.2.3", "2.0.0", false)]
#[case("^1.2.3", "2.0.0-0", false)]
#[case("^1.2.3-rc.1", "1.2.3-rc.15", true)]
#[case("^1.2.3-rc.1", "1.3.0-rc.15", false)]
#[case("^1.2.3-rc.1", "2.0.0-rc.15", false)]

#[case("~1.2.3", "1.2.0", false)]
#[case("~1.2.3", "1.2.3", true)]
#[case("~1.2.3", "1.2.10", true)]
#[case("~1.2.3", "1.2.10-rc", false)]
#[case("~1.2.3", "1.10.0", false)]
#[case("~1.2.3", "2.0.0", false)]

#[case(">1.2.3", "1.2.0", false)]
#[case(">1.2.3", "1.2.3", false)]
#[case(">1.2.3", "1.2.10", true)]
#[case(">1.2.3", "1.10.0", true)]
#[case(">1.2.3", "2.0.0", true)]

#[case("^0.7.0", "0.7.45", true)]
#[case("^0.7.0", "0.8.0", false)]
#[case("^0.7.0", "0.7.3-rc", false)]

#[case("^0.0.3", "0.0.3", true)]
#[case("^0.0.3", "0.0.4", false)]
#[case("^0.0.3", "0.0.4-rc", false)]

#[case(">=1.2.3", "1.2.0", false)]
#[case(">=1.2.3", "1.2.3", true)]
#[case(">=1.2.3", "1.2.10", true)]
#[case(">=1.2.3", "1.10.0", true)]
#[case(">=1.2.3", "2.0.0", true)]

#[case("<1.2.3", "1.2.0", true)]
#[case("<1.2.3", "1.2.3", false)]
#[case("<1.2.3", "1.2.10", false)]
#[case("<1.2.3", "1.10.0", false)]
#[case("<1.2.3", "2.0.0", false)]

#[case("<=1.2.3", "1.2.0", true)]
#[case("<=1.2.3", "1.2.3", true)]
#[case("<=1.2.3", "1.2.10", false)]
#[case("<=1.2.3", "1.10.0", false)]
#[case("<=1.2.3", "2.0.0", false)]

#[case(">=1.2.3 <1.10.3", "1.2.0", false)]
#[case(">=1.2.3 <1.10.3", "1.2.3", true)]
#[case(">=1.2.3 <1.10.3", "1.2.10", true)]
#[case(">=1.2.3 <1.10.3", "1.10.0", true)]
#[case(">=1.2.3 <1.10.3", "1.10.3", false)]

#[case("1.2.3 || 1.2.10", "1.2.0", false)]
#[case("1.2.3 || 1.2.10", "1.2.3", true)]
#[case("1.2.3 || 1.2.10", "1.2.10", true)]
#[case("1.2.3 || 1.2.10", "1.10.0", false)]

#[case("*", "1.2.0", true)]
#[case("x", "1.2.0", true)]
#[case("X", "1.2.0", true)]

#[case("1.*", "1.2.0", true)]
#[case("1.x", "1.2.0", true)]
#[case("1.X", "1.2.0", true)]

#[case("1.2.*", "1.2.0", true)]
#[case("1.2.x", "1.2.0", true)]
#[case("1.2.X", "1.2.0", true)]
fn test_range_check(#[case] range: Range, #[case] version: Version, #[case] expected: bool) {
    assert_eq!(range.check(&version), expected);
}

#[rstest]
#[case("^1.2.3", "1.10.0-rc", true)]
#[case("^1.2.3", "2.0.0-rc", false)]

#[case("~1.2.3", "1.2.10-rc", true)]
#[case("~1.2.3", "1.3.0-rc", false)]
#[case("~1.2.3", "2.0.0-rc", false)]

#[case("^0.7.0", "0.7.3-rc", true)]
#[case("^0.7.0", "0.8.0-rc", false)]

#[case("^0.0.3", "0.0.4-rc", false)]
fn test_range_check_ignore_rc(#[case] range: Range, #[case] version: Version, #[case] expected: bool) {
    assert_eq!(range.check_ignore_rc(version), expected);
}

#[rstest]
#[case("1.2.3", Some(Version { major: 1, minor: 2, patch: 3, rc: None }))]
#[case("1.2.3 || 2.3.4", Some(Version { major: 1, minor: 2, patch: 3, rc: None }))]
#[case("2.3.4 || 1.2.3", Some(Version { major: 1, minor: 2, patch: 3, rc: None }))]
#[case("1.2.3 && 2.3.4", None)]
#[case("1.2.3 2.3.4", None)]
#[case("1.2.3 - 2.3.4", Some(Version { major: 1, minor: 2, patch: 3, rc: None }))]
#[case("1", Some(Version { major: 1, minor: 0, patch: 0, rc: None }))]
#[case("1.2", Some(Version { major: 1, minor: 2, patch: 0, rc: None }))]
#[case("(^1.5 || ^1.2 || ^1.3) && <1.4", Some(Version { major: 1, minor: 2, patch: 0, rc: None }))]
fn test_range_min(#[case] range: Range, #[case] expected: Option<Version>) {
    assert_eq!(range.range_min(), expected);
}

#[rstest]
#[case(Range::caret(Version { major: 1, minor: 2, patch: 3, rc: None }), "^1.2.3")]
#[case(Range::tilde(Version { major: 1, minor: 2, patch: 3, rc: None }), "~1.2.3")]
#[case(Range::exact(Version { major: 1, minor: 2, patch: 3, rc: None }), "1.2.3")]
fn test_range_factories(#[case] range: Range, #[case] expected: Range) {
    assert_eq!(range, expected);
}

#[test]
fn test_range_tokenize() {
    assert_eq!(Range::tokenize("1.2.3"), Some(vec![
        Token::Operation(
            OperatorType::Equal,
            Version::from_file_string("1.2.3").unwrap(),
        ),
    ].into()));

    assert_eq!(Range::tokenize("  1.2.3"), Some(vec![
        Token::Operation(
            OperatorType::Equal,
            Version::from_file_string("1.2.3").unwrap(),
        ),
    ].into()));

    assert_eq!(Range::tokenize("1.2.3   "), Some(vec![
        Token::Operation(
            OperatorType::Equal,
            Version::from_file_string("1.2.3").unwrap(),
        ),
    ].into()));

    assert_eq!(Range::tokenize("1.2.3 || 2.3.4"), Some(vec![
        Token::Operation(
            OperatorType::Equal,
            Version::from_file_string("1.2.3").unwrap(),
        ),
        Token::Syntax(TokenType::Or),
        Token::Operation(
            OperatorType::Equal,
            Version::from_file_string("2.3.4").unwrap(),
        ),
    ].into()));

    assert_eq!(Range::tokenize("1.2.3 && 2.3.4"), Some(vec![
        Token::Operation(
            OperatorType::Equal,
            Version::from_file_string("1.2.3").unwrap(),
        ),
        Token::Syntax(TokenType::And),
        Token::Operation(
            OperatorType::Equal,
            Version::from_file_string("2.3.4").unwrap(),
        ),
    ].into()));

    assert_eq!(Range::tokenize("1.2.3 2.3.4"), Some(vec![
        Token::Operation(
            OperatorType::Equal,
            Version::from_file_string("1.2.3").unwrap(),
        ),
        Token::Syntax(TokenType::SAnd),
        Token::Operation(
            OperatorType::Equal,
            Version::from_file_string("2.3.4").unwrap(),
        ),
    ].into()));

    assert_eq!(Range::tokenize("1.2.3 - 2.3.4"), Some(vec![
        Token::Operation(
            OperatorType::GreaterThanOrEqual,
            Version::from_file_string("1.2.3").unwrap(),
        ),
        Token::Syntax(TokenType::SAnd),
        Token::Operation(
            OperatorType::LessThan,
            Version::from_file_string("2.3.4").unwrap(),
        ),
    ].into()));

    assert_eq!(Range::tokenize("1"), Some(vec![
        Token::Operation(
            OperatorType::GreaterThanOrEqual,
            Version::from_file_string("1.0.0").unwrap(),
        ),
        Token::Syntax(TokenType::SAnd),
        Token::Operation(
            OperatorType::LessThan,
            Version::from_file_string("2.0.0").unwrap(),
        ),
    ].into()));

    assert_eq!(Range::tokenize("1.2"), Some(vec![
        Token::Operation(
            OperatorType::GreaterThanOrEqual,
            Version::from_file_string("1.2.0").unwrap(),
        ),
        Token::Syntax(TokenType::SAnd),
        Token::Operation(
            OperatorType::LessThan,
            Version::from_file_string("1.3.0").unwrap(),
        ),
    ].into()));

    assert_eq!(Range::tokenize("^1.2.3"), Some(vec![
        Token::Operation(
            OperatorType::GreaterThanOrEqual,
            Version::from_file_string("1.2.3").unwrap(),
        ),
        Token::Syntax(TokenType::SAnd),
        Token::Operation(
            OperatorType::LessThan,
            Version::from_file_string("2.0.0-0").unwrap(),
        ),
    ].into()));

    assert_eq!(Range::tokenize("~1.2.3"), Some(vec![
        Token::Operation(
            OperatorType::GreaterThanOrEqual,
            Version::from_file_string("1.2.3").unwrap(),
        ),
        Token::Syntax(TokenType::SAnd),
        Token::Operation(
            OperatorType::LessThan,
            Version::from_file_string("1.3.0-0").unwrap(),
        ),
    ].into()));

    assert_eq!(Range::tokenize("(1.2.3)"), Some(vec![
        Token::Syntax(TokenType::LParen),
        Token::Operation(
            OperatorType::Equal,
            Version::from_file_string("1.2.3").unwrap(),
        ),
        Token::Syntax(TokenType::RParen),
    ].into()));
}
