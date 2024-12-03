use std::borrow::Borrow;

use bincode::{Decode, Encode};

use crate::{error::Error, yarn_serialization_protocol};

use super::{extract, Version};

#[cfg(test)]
#[path = "./range.test.rs"]
mod range_tests;

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

    pub fn check<P: Borrow<Version>>(&self, version: P) -> bool {
        let mut n = 0;

        self.check_from(version.borrow(), &mut n)
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

yarn_serialization_protocol!(Range, "", {
    deserialize(src) {
        let tokens = Range::tokenize(src)
            .ok_or_else(|| Error::InvalidSemverRange(src.to_string()))?;

        let prefix = extract::infix_to_prefix(&tokens)
            .ok_or_else(|| Error::InvalidSemverRange(src.to_string()))?;

        Ok(Range {
            source: src.to_string(),
            tokens: prefix,
        })
    }

    serialize(&self) {
        &self.source
    }
});
