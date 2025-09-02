use std::ops::{Index, Range, RangeFrom, RangeInclusive, RangeTo};

use serde::{de, Deserialize, Deserializer, Serialize};
use zpm_utils::{impl_file_string_from_str, impl_file_string_serialization, DataType, FromFileString, ToFileString, ToHumanString};

use crate::{json::escape_string, Error};

#[derive(Debug, Default, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct Path {
    pub segments: Vec<String>,
}
enum PathSegment<'a> {
    Identifier(&'a str),
    String(&'a str),
    Number(&'a str),
}

impl<'a> PathSegment<'a> {
    fn as_str(&self) -> &str {
        match self {
            PathSegment::Identifier(segment) => segment,
            PathSegment::String(segment) => segment,
            PathSegment::Number(segment) => segment,
        }
    }
}

impl Path {
    pub fn new() -> Self {
        Self {
            segments: Vec::new(),
        }
    }

    pub fn from_segments(segments: Vec<String>) -> Self {
        Self { segments }
    }

    pub fn segments(&self) -> &[String] {
        &self.segments
    }

    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }

    pub fn len(&self) -> usize {
        self.segments.len()
    }

    pub fn last(&self) -> Option<&String> {
        self.segments.last()
    }

    pub fn push(&mut self, segment: String) {
        self.segments.push(segment);
    }

    fn to_parts<'a>(&'a self) -> Vec<PathSegment<'a>> {
        let mut result
            = Vec::new();

        for segment in self.segments.iter() {
            let is_valid_identifier = segment.chars().enumerate().all(|(idx, ch)| {
                if idx == 0 {
                    ch.is_alphabetic() || ch == '_' || ch == '$'
                } else {
                    ch.is_alphanumeric() || ch == '_' || ch == '$'
                }
            });

            if is_valid_identifier {
                result.push(PathSegment::Identifier(segment));
                continue;
            }

            let is_number = segment.chars().all(|ch| {
                ch.is_ascii_digit()
            });

            if is_number {
                result.push(PathSegment::Number(segment));
                continue;
            }

            result.push(PathSegment::String(segment));
        }

        result
    }
}

impl<const N: usize> From<[&str; N]> for Path {
    fn from(path: [&str; N]) -> Self {
        Self::from_segments(path.iter().map(|s| s.to_string()).collect())
    }
}

impl From<Vec<String>> for Path {
    fn from(path: Vec<String>) -> Self {
        Self::from_segments(path)
    }
}

impl Index<usize> for Path {
    type Output = String;

    fn index(&self, index: usize) -> &Self::Output {
        &self.segments[index]
    }
}

impl Index<Range<usize>> for Path {
    type Output = [String];

    fn index(&self, index: Range<usize>) -> &Self::Output {
        &self.segments[index]
    }
}

impl Index<RangeFrom<usize>> for Path {
    type Output = [String];

    fn index(&self, index: RangeFrom<usize>) -> &Self::Output {
        &self.segments[index]
    }
}

impl Index<RangeTo<usize>> for Path {
    type Output = [String];

    fn index(&self, index: RangeTo<usize>) -> &Self::Output {
        &self.segments[index]
    }
}

impl Index<RangeInclusive<usize>> for Path {
    type Output = [String];

    fn index(&self, index: RangeInclusive<usize>) -> &Self::Output {
        &self.segments[index]
    }
}

impl FromFileString for Path {
    type Error = Error;

    fn from_file_string(src: &str) -> Result<Self, Error> {
        let mut segments
            = Vec::new();
        let mut chars
            = src.chars();
        let mut current_segment
            = String::new();

        while let Some(ch) = chars.next() {
            match ch {
                '.' => {
                    if !current_segment.is_empty() {
                        segments.push(current_segment);
                        current_segment = String::new();
                    }
                }

                '[' => {
                    if !current_segment.is_empty() {
                        segments.push(current_segment);
                        current_segment = String::new();
                    }

                    // Check if it's a string or number
                    match chars.next() {
                        Some(quote_char) if quote_char == '"' || quote_char == '\'' => {
                            let mut escaped
                                = false;

                            while let Some(ch) = chars.next() {
                                if escaped {
                                    current_segment.push(ch);
                                    escaped = false;
                                } else if ch == '\\' {
                                    escaped = true;
                                } else if ch == quote_char {
                                    break;
                                } else {
                                    current_segment.push(ch);
                                }
                            }

                            if chars.next() != Some(']') {
                                return Err(Error::InvalidSyntax("Expected ']' after quoted string".to_string()));
                            }

                            segments.push(current_segment);
                            current_segment = String::new();
                        },

                        // Numeric index
                        Some(numeric_char) if numeric_char.is_ascii_digit() => {
                            current_segment.push(numeric_char);
                            let mut found_closing_bracket = false;

                            while let Some(ch) = chars.next() {
                                if ch.is_ascii_digit() {
                                    current_segment.push(ch);
                                } else if ch == ']' {
                                    found_closing_bracket = true;
                                    break;
                                } else {
                                    break;
                                }
                            }

                            if !found_closing_bracket {
                                return Err(Error::InvalidSyntax("Expected ']' after number".to_string()));
                            }

                            segments.push(current_segment);
                            current_segment = String::new();
                        },

                        _ => {
                            return Err(Error::InvalidSyntax("Invalid bracket notation".to_string()))
                        },
                    }
                },

                _ => {
                    current_segment.push(ch);
                },
            }
        }

        if !current_segment.is_empty() {
            segments.push(current_segment);
        }

        Ok(Self { segments })
    }
}

impl ToFileString for Path {
    fn to_file_string(&self) -> String {
        let parts
            = self.to_parts();

        let guessed_size
            = parts.iter().map(|part| part.as_str().len()).sum::<usize>()
            + parts.len() * 4;

        let mut result
            = String::with_capacity(guessed_size);

        for part in parts {
            match part {
                PathSegment::Identifier(segment) => {
                    if !result.is_empty() {
                        result.push_str(".");
                    }

                    result.push_str(segment);
                },

                PathSegment::Number(segment) => {
                    result.push_str("[");
                    result.push_str(segment);
                    result.push_str("]");
                },

                PathSegment::String(segment) => {
                    result.push_str("[");
                    result.push_str(&escape_string(segment));
                    result.push_str("]");
                },
            }
        }

        result
    }
}

impl ToHumanString for Path {
    fn to_print_string(&self) -> String {
        let parts
            = self.to_parts();

        let guessed_size
            = parts.iter().map(|part| part.as_str().len()).sum::<usize>()
            + parts.len() * 4;

        let mut result
            = String::with_capacity(guessed_size);

        for part in parts {
            match part {
                PathSegment::Identifier(segment) => {
                    if !result.is_empty() {
                        result.push_str(&DataType::Code.colorize("."));
                    }

                    result.push_str(segment);
                },

                PathSegment::Number(segment) => {
                    result.push_str(&DataType::Code.colorize("["));
                    result.push_str(&DataType::Number.colorize(segment));
                    result.push_str(&DataType::Code.colorize("]"));
                },

                PathSegment::String(segment) => {
                    result.push_str(&DataType::Code.colorize("["));
                    result.push_str(&DataType::String.colorize(&escape_string(segment)));
                    result.push_str(&DataType::Code.colorize("]"));
                },
            }
        }

        result
    }
}

impl_file_string_from_str!(Path);
impl_file_string_serialization!(Path);
