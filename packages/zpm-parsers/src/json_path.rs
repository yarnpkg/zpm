use serde::{de, Deserialize, Deserializer};
use zpm_utils::{impl_serialization_traits, impl_serialization_traits_no_serde, DataType, FromFileString, ToFileString, ToHumanString};

use crate::{json::escape_string, Error};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct JsonPath {
    pub segments: Vec<String>,
}

enum JsonPathPart<'a> {
    Identifier(&'a str),
    String(&'a str),
    Number(&'a str),
}

impl<'a> JsonPathPart<'a> {
    fn as_str(&self) -> &str {
        match self {
            JsonPathPart::Identifier(segment) => segment,
            JsonPathPart::String(segment) => segment,
            JsonPathPart::Number(segment) => segment,
        }
    }
}

impl JsonPath {
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

    fn to_parts<'a>(&'a self) -> Vec<JsonPathPart<'a>> {
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
                result.push(JsonPathPart::Identifier(segment));
                continue;
            }

            let is_number = segment.chars().all(|ch| {
                ch.is_ascii_digit()
            });

            if is_number {
                result.push(JsonPathPart::Number(segment));
                continue;
            }

            result.push(JsonPathPart::String(segment));
        }

        result
    }
}

impl From<Vec<String>> for JsonPath {
    fn from(segments: Vec<String>) -> Self {
        Self::from_segments(segments)
    }
}

impl From<Vec<&str>> for JsonPath {
    fn from(segments: Vec<&str>) -> Self {
        Self::from_segments(segments.into_iter().map(|s| s.to_string()).collect())
    }
}

impl FromFileString for JsonPath {
    type Error = Error;

    fn from_file_string(src: &str) -> Result<Self, Error> {
        let mut segments = Vec::new();
        let mut chars = src.chars().peekable();
        let mut current_segment = String::new();

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
                    match chars.peek() {
                        Some('"') | Some('\'') => {
                            // String index
                            let quote_char = chars.next().unwrap();
                            let mut escaped = false;
                            
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

                            // Expect closing bracket
                            match chars.next() {
                                Some(']') => {
                                    segments.push(current_segment);
                                    current_segment = String::new();
                                }
                                _ => return Err(Error::InvalidSyntax("Expected ']' after quoted string".to_string())),
                            }
                        },

                        Some('0'..='9') => {
                            // Numeric index
                            while let Some(ch) = chars.peek() {
                                if ch.is_ascii_digit() {
                                    current_segment.push(chars.next().unwrap());
                                } else {
                                    break;
                                }
                            }

                            // Expect closing bracket
                            match chars.next() {
                                Some(']') => {
                                    segments.push(current_segment);
                                    current_segment = String::new();
                                }
                                _ => return Err(Error::InvalidSyntax("Expected ']' after number".to_string())),
                            }
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

impl ToFileString for JsonPath {
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
                JsonPathPart::Identifier(segment) => {
                    if !result.is_empty() {
                        result.push_str(".");
                    }

                    result.push_str(segment);
                },

                JsonPathPart::Number(segment) => {
                    result.push_str("[");
                    result.push_str(segment);
                    result.push_str("]");
                },

                JsonPathPart::String(segment) => {
                    result.push_str("[");
                    result.push_str(segment);
                    result.push_str("]");
                },
            }
        }

        result
    }
}

impl ToHumanString for JsonPath {
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
                JsonPathPart::Identifier(segment) => {
                    if !result.is_empty() {
                        result.push_str(&DataType::Code.colorize("."));
                    }

                    result.push_str(segment);
                },

                JsonPathPart::Number(segment) => {
                    result.push_str(&DataType::Code.colorize("["));
                    result.push_str(&DataType::Number.colorize(segment));
                    result.push_str(&DataType::Code.colorize("]"));
                },

                JsonPathPart::String(segment) => {
                    result.push_str(&DataType::Code.colorize("["));
                    result.push_str(&DataType::String.colorize(&escape_string(segment)));
                    result.push_str(&DataType::Code.colorize("]"));
                },
            }
        }

        result
    }
}

impl_serialization_traits_no_serde!(JsonPath);

#[derive(Deserialize)]
#[serde(untagged)]
enum JsonPathDeserializer {
    String(String),
    Array(Vec<String>),
}

impl<'de> Deserialize<'de> for JsonPath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> where D: Deserializer<'de> {
        let deserialized
            = JsonPathDeserializer::deserialize(deserializer)?;

        match deserialized {
            JsonPathDeserializer::String(path) => {
                Ok(JsonPath::from_file_string(&path).map_err(de::Error::custom)?)
            },

            JsonPathDeserializer::Array(segments) => {
                Ok(JsonPath::from_segments(segments))
            },
        }
    }
}
