use std::str::FromStr;

use rstest::rstest;

use crate::semver::{range::{OperatorType, Token, TokenType}, Range, Version};

#[rstest]
#[case("1.2.3", "1.2.3", true)]

#[case("^1.2.3", "1.2.0", false)]
#[case("^1.2.3", "1.2.3", true)]
#[case("^1.2.3", "1.2.10", true)]
#[case("^1.2.3", "1.10.0", true)]
#[case("^1.2.3", "2.0.0-rc", false)]
#[case("^1.2.3", "2.0.0-0", false)]
#[case("^1.2.3", "2.0.0", false)]

#[case("~1.2.3", "1.2.0", false)]
#[case("~1.2.3", "1.2.3", true)]
#[case("~1.2.3", "1.2.10", true)]
#[case("~1.2.3", "1.10.0", false)]
#[case("~1.2.3", "2.0.0", false)]

#[case(">1.2.3", "1.2.0", false)]
#[case(">1.2.3", "1.2.3", false)]
#[case(">1.2.3", "1.2.10", true)]
#[case(">1.2.3", "1.10.0", true)]
#[case(">1.2.3", "2.0.0", true)]

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
    assert_eq!(range.check(version), expected);
}

#[test]
fn test_range_tokenize() {
    assert_eq!(Range::tokenize("1.2.3"), Some(vec![
        Token::Operation(
            OperatorType::Equal,
            Version::from_str("1.2.3").unwrap(),
        ),
    ]));

    assert_eq!(Range::tokenize("  1.2.3"), Some(vec![
        Token::Operation(
            OperatorType::Equal,
            Version::from_str("1.2.3").unwrap(),
        ),
    ]));

    assert_eq!(Range::tokenize("1.2.3   "), Some(vec![
        Token::Operation(
            OperatorType::Equal,
            Version::from_str("1.2.3").unwrap(),
        ),
    ]));

    assert_eq!(Range::tokenize("1.2.3 || 2.3.4"), Some(vec![
        Token::Operation(
            OperatorType::Equal,
            Version::from_str("1.2.3").unwrap(),
        ),
        Token::Syntax(TokenType::Or),
        Token::Operation(
            OperatorType::Equal,
            Version::from_str("2.3.4").unwrap(),
        ),
    ]));

    assert_eq!(Range::tokenize("1.2.3 && 2.3.4"), Some(vec![
        Token::Operation(
            OperatorType::Equal,
            Version::from_str("1.2.3").unwrap(),
        ),
        Token::Syntax(TokenType::And),
        Token::Operation(
            OperatorType::Equal,
            Version::from_str("2.3.4").unwrap(),
        ),
    ]));

    assert_eq!(Range::tokenize("1.2.3 2.3.4"), Some(vec![
        Token::Operation(
            OperatorType::Equal,
            Version::from_str("1.2.3").unwrap(),
        ),
        Token::Syntax(TokenType::SAnd),
        Token::Operation(
            OperatorType::Equal,
            Version::from_str("2.3.4").unwrap(),
        ),
    ]));

    assert_eq!(Range::tokenize("1.2.3 - 2.3.4"), Some(vec![
        Token::Operation(
            OperatorType::GreaterThanOrEqual,
            Version::from_str("1.2.3").unwrap(),
        ),
        Token::Syntax(TokenType::SAnd),
        Token::Operation(
            OperatorType::LessThan,
            Version::from_str("2.3.4").unwrap(),
        ),
    ]));

    assert_eq!(Range::tokenize("1"), Some(vec![
        Token::Operation(
            OperatorType::GreaterThanOrEqual,
            Version::from_str("1.0.0").unwrap(),
        ),
        Token::Syntax(TokenType::SAnd),
        Token::Operation(
            OperatorType::LessThan,
            Version::from_str("2.0.0").unwrap(),
        ),
    ]));

    assert_eq!(Range::tokenize("1.2"), Some(vec![
        Token::Operation(
            OperatorType::GreaterThanOrEqual,
            Version::from_str("1.2.0").unwrap(),
        ),
        Token::Syntax(TokenType::SAnd),
        Token::Operation(
            OperatorType::LessThan,
            Version::from_str("1.3.0").unwrap(),
        ),
    ]));

    assert_eq!(Range::tokenize("^1.2.3"), Some(vec![
        Token::Operation(
            OperatorType::GreaterThanOrEqual,
            Version::from_str("1.2.3").unwrap(),
        ),
        Token::Syntax(TokenType::SAnd),
        Token::Operation(
            OperatorType::LessThan,
            Version::from_str("2.0.0-0").unwrap(),
        ),
    ]));

    assert_eq!(Range::tokenize("~1.2.3"), Some(vec![
        Token::Operation(
            OperatorType::GreaterThanOrEqual,
            Version::from_str("1.2.3").unwrap(),
        ),
        Token::Syntax(TokenType::SAnd),
        Token::Operation(
            OperatorType::LessThan,
            Version::from_str("1.3.0-0").unwrap(),
        ),
    ]));

    assert_eq!(Range::tokenize("(1.2.3)"), Some(vec![
        Token::Syntax(TokenType::LParen),
        Token::Operation(
            OperatorType::Equal,
            Version::from_str("1.2.3").unwrap(),
        ),
        Token::Syntax(TokenType::RParen),
    ]));
}
