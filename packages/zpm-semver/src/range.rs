use std::borrow::Borrow;
use bincode::{Decode, Encode};
use zpm_utils::{impl_file_string_from_str, impl_file_string_serialization, FromFileString, ToFileString, ToHumanString};

use crate::{Error, VersionRc};

use super::{extract, Version};

#[cfg(test)]
#[path = "./range.test.rs"]
mod range_tests;

#[derive(Clone, Copy, Debug, Decode, Encode, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RangeKind {
    Caret,
    Tilde,
    Exact,
}

impl FromFileString for RangeKind {
    type Error = Error;

    fn from_file_string(raw: &str) -> Result<Self, Self::Error> {
        match raw {
            "^" | "caret" => Ok(RangeKind::Caret),
            "~" | "tilde" => Ok(RangeKind::Tilde),
            "=" | "exact" | "*" | "" => Ok(RangeKind::Exact),
            _ => Err(Error::InvalidRange(raw.to_string())),
        }
    }
}

impl ToFileString for RangeKind {
    fn to_file_string(&self) -> String {
        match self {
            RangeKind::Caret => "^".to_string(),
            RangeKind::Tilde => "~".to_string(),
            RangeKind::Exact => "*".to_string(),
        }
    }
}

impl ToHumanString for RangeKind {
    fn to_print_string(&self) -> String {
        match self {
            RangeKind::Caret => "^".to_string(),
            RangeKind::Tilde => "~".to_string(),
            RangeKind::Exact => "=".to_string(),
        }
    }
}

impl_file_string_from_str!(RangeKind);
impl_file_string_serialization!(RangeKind);

#[derive(Clone, Copy, Debug, Decode, Encode, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum TokenType {
    LParen,
    RParen,
    SAnd,
    And,
    Or,
}

#[derive(Clone, Copy, Debug, Decode, Encode, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum OperatorType {
    Equal,
    LessThan,
    GreaterThan,
    LessThanOrEqual,
    GreaterThanOrEqual,
}

#[derive(Clone, Debug, Decode, Encode, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Token {
    Syntax(TokenType),
    Operation(OperatorType, Version),
}

#[derive(Clone, Debug, Decode, Encode, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Range {
    pub source: String,

    tokens: Vec<Token>,
}

impl Range {
    fn tokenize<P: AsRef<str>>(str: P) -> Option<Vec<Token>> {
        extract::extract_tokens(&mut str.as_ref().chars().peekable())
    }

    pub fn any() -> Range {
        Range {
            // TODO: Replace >=0.0.0-0 with "*" once "*" is implemented
            source: ">=0.0.0-0".to_string(),
            tokens: vec![
                Token::Operation(OperatorType::GreaterThanOrEqual, Version::new_from_components(0, 0, 0, Some(vec![VersionRc::Number(0)]))),
            ],
        }
    }

    pub fn caret(version: Version) -> Range {
        let upper_bound = match (version.major, version.minor) {
            (0, 0) => version.next_patch_rc(),
            (0, _) => version.next_minor_rc(),
            _ => version.next_major_rc(),
        };

        Range {
            source: format!("^{}", version.to_file_string()),
            tokens: vec![
                Token::Syntax(TokenType::SAnd),
                Token::Operation(OperatorType::GreaterThanOrEqual, version),
                Token::Operation(OperatorType::LessThan, upper_bound),
            ],
        }
    }

    pub fn tilde(version: Version) -> Range {
        let upper_bound
            = version.next_minor_rc();

        Range {
            source: format!("~{}", version.to_file_string()),
            tokens: vec![
                Token::Syntax(TokenType::SAnd),
                Token::Operation(OperatorType::GreaterThanOrEqual, version),
                Token::Operation(OperatorType::LessThan, upper_bound),
            ],
        }
    }

    pub fn exact(version: Version) -> Range {
        Range {
            source: version.to_file_string(),
            tokens: vec![Token::Operation(OperatorType::Equal, version)],
        }
    }

    pub fn kind(&self) -> Option<RangeKind> {
        match self.source.chars().next() {
            Some('0' | '1' | '2' | '3' | '4' | '5' | '6' | '7' | '8' | '9')
                => Some(RangeKind::Exact),

            Some('^') => Some(RangeKind::Caret),
            Some('~') => Some(RangeKind::Tilde),

            _ => None,
        }
    }

    pub fn check<P: Borrow<Version>>(&self, version: P) -> bool {
        let mut n = 0;

        let version
            = version.borrow();

        // https://docs.npmjs.com/cli/v6/using-npm/semver#prerelease-tags
        //
        // > a version has a prerelease tag (for example, 1.2.3-alpha.3) then it
        // > will only be allowed to satisfy comparator sets if at least one
        // > comparator with the same [major, minor, patch] tuple also has
        // > a prerelease tag.
        // >
        // > For example, the range >1.2.3-alpha.3 would be allowed to match
        // > the version 1.2.3-alpha.7, but it would not be satisfied by
        // > 3.4.5-alpha.9, even though 3.4.5-alpha.9 is technically "greater
        // > than" 1.2.3-alpha.3 according to the SemVer sort rules. The version
        // > range only accepts prerelease tags on the 1.2.3 version. The
        // > version 3.4.5 would satisfy the range, because it does not have a
        // > prerelease flag, and 3.4.5 is greater than 1.2.3-alpha.7.
        //
        if version.rc.is_some() && !self.tokens.iter().any(|t| matches!(t, Token::Operation(_, operand) if operand.major == version.major && operand.minor == version.minor && operand.patch == version.patch && operand.rc.is_some())) {
            return false;
        }

        self.check_from(version, &mut n, false)
    }

    pub fn check_ignore_rc<P: Borrow<Version>>(&self, version: P) -> bool {
        let mut n = 0;

        self.check_from(version.borrow(), &mut n, true)
    }

    fn check_from(&self, version: &Version, n: &mut usize, accept_rc: bool) -> bool {
        let token = self.tokens.get(*n);
        *n += 1;

        match token {
            Some(Token::Syntax(TokenType::SAnd)) | Some(Token::Syntax(TokenType::And)) => {
                let left = self.check_from(version, n, accept_rc);
                let right = self.check_from(version, n, accept_rc);

                left && right
            }

            Some(Token::Syntax(TokenType::Or)) => {
                let left = self.check_from(version, n, accept_rc);
                let right = self.check_from(version, n, accept_rc);

                left || right
            }

            Some(Token::Operation(OperatorType::Equal, operand)) => {
                version == operand
            }

            Some(Token::Operation(OperatorType::GreaterThan, operand)) => {
                version > operand
            }

            Some(Token::Operation(OperatorType::GreaterThanOrEqual, operand)) => {
                version >= operand
            }

            Some(Token::Operation(OperatorType::LessThan, operand)) => {
                version < operand
            }

            Some(Token::Operation(OperatorType::LessThanOrEqual, operand)) => {
                version <= operand
            }

            _ => {
                unreachable!();
            }
        }
    }

    pub fn range_min(&self) -> Option<Version> {
        let mut n = 0;

        self.min_from(&mut n)
            .into_iter()
            .filter(|v| self.check(v))
            .min()
    }

    fn min_from(&self, n: &mut usize) -> Vec<Version> {
        let token = self.tokens.get(*n);
        *n += 1;

        match token {
            Some(Token::Syntax(TokenType::SAnd)) | Some(Token::Syntax(TokenType::And)) => {
                let left = self.min_from(n);
                let right = self.min_from(n);

                left.into_iter().chain(right).collect()
            }

            Some(Token::Syntax(TokenType::Or)) => {
                let left = self.min_from(n);
                let right = self.min_from(n);

                left.into_iter().chain(right).collect()
            }

            Some(Token::Operation(OperatorType::Equal, operand)) => {
                vec![operand.clone()]
            }

            Some(Token::Operation(OperatorType::GreaterThan, operand)) => {
                vec![operand.next_immediate_spec()]
            }

            Some(Token::Operation(OperatorType::GreaterThanOrEqual, operand)) => {
                vec![operand.clone()]
            }

            Some(Token::Operation(OperatorType::LessThan, ..)) => {
                vec![]
            }

            Some(Token::Operation(OperatorType::LessThanOrEqual, ..)) => {
                vec![]
            }

            _ => {
                unreachable!();
            }
        }
    }
}

impl FromFileString for Range {
    type Error = Error;

    fn from_file_string(src: &str) -> Result<Self, Error> {
        let tokens = Range::tokenize(src)
            .ok_or_else(|| Error::InvalidRange(src.to_string()))?;

        let prefix = extract::infix_to_prefix(&tokens)
            .ok_or_else(|| Error::InvalidRange(src.to_string()))?;

        Ok(Range {
            source: src.to_string(),
            tokens: prefix,
        })
    }
}

impl ToFileString for Range {
    fn to_file_string(&self) -> String {
        self.source.clone()
    }
}

impl ToHumanString for Range {
    fn to_print_string(&self) -> String {
        self.to_file_string()
    }
}

impl_file_string_from_str!(Range);
impl_file_string_serialization!(Range);
