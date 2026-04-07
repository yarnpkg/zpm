use std::fmt;

use version_ranges::Ranges;

use crate::range::{OperatorType, Token, TokenType};
use crate::{Range, Version};

/// Newtype wrapper around [`Version`] that implements [`Display`]
/// for use with pubgrub's [`Ranges<V>`] and [`DependencyProvider`].
///
/// `Version` itself intentionally does not implement `Display`;
/// this wrapper is the only path to a human-readable representation
/// in the pubgrub context.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PubgrubVersion(pub Version);

impl PubgrubVersion {
    pub fn new(version: Version) -> Self {
        Self(version)
    }

    pub fn into_inner(self) -> Version {
        self.0
    }
}

impl From<Version> for PubgrubVersion {
    fn from(v: Version) -> Self {
        Self(v)
    }
}

impl Default for PubgrubVersion {
    fn default() -> Self {
        Self(Version::default())
    }
}

impl fmt::Display for PubgrubVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use zpm_utils::ToFileString;
        write!(f, "{}", self.0.to_file_string())
    }
}

impl std::ops::Deref for PubgrubVersion {
    type Target = Version;

    fn deref(&self) -> &Version {
        &self.0
    }
}

/// Extension trait to convert a [`Range`] into pubgrub's [`Ranges<PubgrubVersion>`].
pub trait ToRanges {
    fn to_ranges(&self) -> Ranges<PubgrubVersion>;
}

impl ToRanges for Range {
    fn to_ranges(&self) -> Ranges<PubgrubVersion> {
        let tokens = self.tokens();
        if tokens.is_empty() {
            return Ranges::full();
        }

        let mut idx = 0;
        convert_tokens(tokens, &mut idx)
    }
}

fn convert_tokens(tokens: &[Token], idx: &mut usize) -> Ranges<PubgrubVersion> {
    let token = tokens.get(*idx);
    *idx += 1;

    match token {
        Some(Token::Syntax(TokenType::SAnd)) | Some(Token::Syntax(TokenType::And)) => {
            let left = convert_tokens(tokens, idx);
            let right = convert_tokens(tokens, idx);
            left.intersection(&right)
        }

        Some(Token::Syntax(TokenType::Or)) => {
            let left = convert_tokens(tokens, idx);
            let right = convert_tokens(tokens, idx);
            left.union(&right)
        }

        Some(Token::Operation(OperatorType::Equal, version)) => {
            Ranges::singleton(PubgrubVersion(version.clone()))
        }

        Some(Token::Operation(OperatorType::GreaterThan, version)) => {
            Ranges::strictly_higher_than(PubgrubVersion(version.clone()))
        }

        Some(Token::Operation(OperatorType::GreaterThanOrEqual, version)) => {
            Ranges::higher_than(PubgrubVersion(version.clone()))
        }

        Some(Token::Operation(OperatorType::LessThan, version)) => {
            Ranges::strictly_lower_than(PubgrubVersion(version.clone()))
        }

        Some(Token::Operation(OperatorType::LessThanOrEqual, version)) => {
            Ranges::strictly_lower_than(PubgrubVersion(version.clone()))
                .union(&Ranges::singleton(PubgrubVersion(version.clone())))
        }

        Some(Token::Syntax(TokenType::LParen)) | Some(Token::Syntax(TokenType::RParen)) => {
            convert_tokens(tokens, idx)
        }

        None => {
            Ranges::full()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zpm_utils::FromFileString;

    fn v(s: &str) -> PubgrubVersion {
        PubgrubVersion(Version::from_file_string(s).unwrap())
    }

    fn r(s: &str) -> Range {
        Range::from_file_string(s).unwrap()
    }

    #[test]
    fn test_exact_version() {
        let range = r("1.2.3").to_ranges();
        assert!(range.contains(&v("1.2.3")));
        assert!(!range.contains(&v("1.2.4")));
        assert!(!range.contains(&v("1.2.2")));
    }

    #[test]
    fn test_caret_range() {
        let range = r("^1.2.3").to_ranges();
        assert!(range.contains(&v("1.2.3")));
        assert!(range.contains(&v("1.9.0")));
        assert!(!range.contains(&v("2.0.0")));
        assert!(!range.contains(&v("1.2.2")));
    }

    #[test]
    fn test_tilde_range() {
        let range = r("~1.2.3").to_ranges();
        assert!(range.contains(&v("1.2.3")));
        assert!(range.contains(&v("1.2.9")));
        assert!(!range.contains(&v("1.3.0")));
    }

    #[test]
    fn test_gte_range() {
        let range = r(">=1.0.0").to_ranges();
        assert!(range.contains(&v("1.0.0")));
        assert!(range.contains(&v("2.0.0")));
        assert!(!range.contains(&v("0.9.9")));
    }

    #[test]
    fn test_any_range() {
        let range = Range::any().to_ranges();
        assert!(range.contains(&v("0.0.0")));
        assert!(range.contains(&v("999.999.999")));
    }
}
