use std::borrow::Borrow;
use zpm_utils::{impl_serialization_traits, FromFileString, ToFileString, ToHumanString};

use crate::Error;

use super::{extract, Version};

#[cfg(test)]
#[path = "./range.test.rs"]
mod range_tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RangeKind {
    Caret,
    Tilde,
    Exact,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "bincode", derive(bincode_derive::Decode, bincode_derive::Encode))]
pub enum TokenType {
    LParen,
    RParen,
    SAnd,
    And,
    Or,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "bincode", derive(bincode_derive::Decode, bincode_derive::Encode))]
pub enum OperatorType {
    Equal,
    LessThan,
    GreaterThan,
    LessThanOrEqual,
    GreaterThanOrEqual,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "bincode", derive(bincode_derive::Decode, bincode_derive::Encode))]
pub enum Token {
    Syntax(TokenType),
    Operation(OperatorType, Version),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "bincode", derive(bincode_derive::Decode, bincode_derive::Encode))]
pub struct Range {
    pub source: String,

    tokens: Vec<Token>,
}

impl Range {
    fn tokenize<P: AsRef<str>>(str: P) -> Option<Vec<Token>> {
        extract::extract_tokens(&mut str.as_ref().chars().peekable())
    }

    pub fn check<P: Borrow<Version>>(&self, version: P) -> bool {
        let mut n = 0;

        self.check_from(version.borrow(), &mut n)
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

    fn check_from(&self, version: &Version, n: &mut usize) -> bool {
        let token = self.tokens.get(*n);
        *n += 1;

        match token {
            Some(Token::Syntax(TokenType::SAnd)) | Some(Token::Syntax(TokenType::And)) => {
                let left = self.check_from(version, n);
                let right = self.check_from(version, n);

                left && right
            }

            Some(Token::Syntax(TokenType::Or)) => {
                let left = self.check_from(version, n);
                let right = self.check_from(version, n);

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

impl_serialization_traits!(Range);
