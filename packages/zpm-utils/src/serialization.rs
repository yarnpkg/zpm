use std::{fmt, string::FromUtf8Error};

use colored::Colorize;
use crate::EcoString;
use erased_serde::serialize_trait_object;
use fundu::parse_duration;
use serde::{Serialize, Serializer};
use thiserror::Error;

use crate::DataType;

#[derive(Error, Debug)]
pub enum SerializationError {
    #[error("Invalid value: {0}")]
    InvalidValue(String),
}

pub trait FromFileString {
    type Error;

    fn from_file_string(s: &str) -> Result<Self, Self::Error>
        where Self: Sized;
}

pub trait ToFileString {
    fn to_file_string(&self) -> String;
}

pub trait ToHumanString {
    fn to_print_string(&self) -> String;
}

pub trait Extracted: erased_serde::Serialize + ToHumanString + fmt::Debug {
}

impl<T: erased_serde::Serialize + ToHumanString + fmt::Debug> Extracted for T {
}

serialize_trait_object!(Extracted);

pub struct AbstractValue<'a> {
    value: Box<dyn Extracted + 'a>,
    is_container: bool,
}

impl<'a> AbstractValue<'a> {
    pub fn new<T: Extracted + 'a>(value: T) -> Self {
        Self {value: Box::new(value), is_container: false}
    }

    pub fn new_container<T: Extracted + 'a>(value: T) -> Self {
        Self {value: Box::new(value), is_container: true}
    }

    pub fn is_container(&self) -> bool {
        self.is_container
    }

    pub fn export(self, json: bool) -> String {
        if json {
            crate::internal::to_json_string(&self.value)
        } else {
            self.value.to_print_string()
        }
    }
}

impl<'a> fmt::Debug for AbstractValue<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.value.fmt(f)
    }
}

impl<'a> Serialize for AbstractValue<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where S: Serializer {
        self.value.serialize(serializer)
    }
}

impl<'a> ToHumanString for AbstractValue<'a> {
    fn to_print_string(&self) -> String {
        self.value.to_print_string()
    }
}

#[derive(Debug)]
pub struct Container<T> {
    value: T,
}

impl<T> Container<T> {
    pub fn new(value: T) -> Self {
        Self {value}
    }
}

impl<T: Serialize> Serialize for Container<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where S: Serializer {
        self.value.serialize(serializer)
    }
}

impl<T> ToHumanString for Container<T> {
    fn to_print_string(&self) -> String {
        String::from("unimplemented; use --json to display container contents for now")
    }
}

impl<T: FromFileString> FromFileString for Box<T> {
    type Error = <T as FromFileString>::Error;

    fn from_file_string(s: &str) -> Result<Self, Self::Error> {
        Ok(Box::new(T::from_file_string(s)?))
    }
}

impl<T: ToFileString> ToFileString for Box<T> {
    fn to_file_string(&self) -> String {
        self.as_ref().to_file_string()
    }
}

impl<T: ToHumanString> ToHumanString for Box<T> {
    fn to_print_string(&self) -> String {
        self.as_ref().to_print_string()
    }
}

impl FromFileString for bool {
    type Error = SerializationError;

    fn from_file_string(s: &str) -> Result<Self, Self::Error> {
        match s {
            "true" | "1" => {
                Ok(true)
            },

            "false" | "0" => {
                Ok(false)
            },

            _ => {
                Err(SerializationError::InvalidValue(s.to_string()))
            },
        }
    }
}

impl FromFileString for EcoString {
    type Error = SerializationError;

    fn from_file_string(s: &str) -> Result<Self, Self::Error> {
        Ok(EcoString::from(s))
    }
}

impl ToFileString for EcoString {
    fn to_file_string(&self) -> String {
        self.as_str().to_string()
    }
}

impl ToFileString for bool {
    fn to_file_string(&self) -> String {
        self.to_string()
    }
}

impl ToHumanString for bool {
    fn to_print_string(&self) -> String {
        DataType::Boolean.colorize(&self.to_file_string())
    }
}

impl FromFileString for usize {
    type Error = std::num::ParseIntError;

    fn from_file_string(s: &str) -> Result<Self, Self::Error> {
        s.parse()
    }
}

impl ToFileString for usize {
    fn to_file_string(&self) -> String {
        self.to_string()
    }
}

impl ToHumanString for usize {
    fn to_print_string(&self) -> String {
        DataType::Number.colorize(&self.to_file_string())
    }
}

impl FromFileString for u64 {
    type Error = std::num::ParseIntError;

    fn from_file_string(s: &str) -> Result<Self, Self::Error> {
        s.parse()
    }
}

impl ToFileString for u64 {
    fn to_file_string(&self) -> String {
        self.to_string()
    }
}

impl ToHumanString for u64 {
    fn to_print_string(&self) -> String {
        DataType::Number.colorize(&self.to_file_string())
    }
}

impl FromFileString for std::time::Duration {
    type Error = fundu::ParseError;

    fn from_file_string(s: &str) -> Result<Self, Self::Error> {
        parse_duration(s)
    }
}

/// Durations are written as a single integer followed by the largest unit
/// that represents them exactly, from milliseconds to days (`0s`, `90s`,
/// `3h`, `1d`, `36h`, `1500ms`). The unit is always explicit so that the
/// result doesn't depend on how a bare number would be interpreted, and the
/// units are the ones Yarn 4 also understands. Weeks aren't used, since `7d`
/// and `14d` are easier to read than `1w` and `2w`. Durations with a
/// sub-millisecond part fall back to nanoseconds to stay exact.
impl ToFileString for std::time::Duration {
    fn to_file_string(&self) -> String {
        const UNITS: [(&str, u128); 4] = [
            ("d", 24 * 60 * 60 * 1000),
            ("h", 60 * 60 * 1000),
            ("m", 60 * 1000),
            ("s", 1000),
        ];

        if self.is_zero() {
            return "0s".to_string();
        }

        if self.subsec_nanos() % 1_000_000 != 0 {
            return format!("{}ns", self.as_nanos());
        }

        let ms
            = self.as_millis();

        for (unit, multiplier) in UNITS {
            if ms % multiplier == 0 {
                return format!("{}{}", ms / multiplier, unit);
            }
        }

        format!("{}ms", ms)
    }
}

impl ToHumanString for std::time::Duration {
    fn to_print_string(&self) -> String {
        DataType::Number.colorize(&self.to_file_string())
    }
}

impl FromFileString for String {
    type Error = FromUtf8Error;

    fn from_file_string(s: &str) -> Result<Self, Self::Error> {
        Ok(s.to_string())
    }
}

impl ToFileString for String {
    fn to_file_string(&self) -> String {
        self.as_str().to_file_string()
    }
}

impl ToHumanString for String {
    fn to_print_string(&self) -> String {
        DataType::String.colorize(&self.to_file_string())
    }
}

impl ToFileString for &str {
    fn to_file_string(&self) -> String {
        self.to_string()
    }
}

impl ToHumanString for &str {
    fn to_print_string(&self) -> String {
        self.to_file_string().truecolor(0, 153, 0).to_string()
    }
}

impl<T: FromFileString> FromFileString for Option<T> {
    type Error = <T as FromFileString>::Error;

    fn from_file_string(s: &str) -> Result<Self, Self::Error> {
        if s == "null" {
            return Ok(None);
        }

        Ok(Some(T::from_file_string(s)?))
    }
}

impl<T: ToFileString> ToFileString for Option<T> {
    fn to_file_string(&self) -> String {
        if let Some(value) = self {
            value.to_file_string()
        } else {
            "null".to_string()
        }
    }
}

impl<T: ToHumanString> ToHumanString for Option<T> {
    fn to_print_string(&self) -> String {
        if let Some(value) = self {
            value.to_print_string()
        } else {
            DataType::Null.colorize("null")
        }
    }
}

/**
 * This macro implements the `FromStr` and similar traits for a type that
 * implements `FromFileString`. Ideally we wouldn't use that, as the zpm
 * code is supposed to use FromFileString.
 *
 * In some cases we need to interact with third-party libraries that rely
 * on FromStr (for example Clipanion, since relying on FromFileString
 * wouldn't make sense there), in which case the relevant types must be
 * annotated.
 */
#[macro_export]
macro_rules! impl_file_string_from_str(($type:ty) => {
    impl std::str::FromStr for $type {
        type Err = <$type as $crate::FromFileString>::Error;

        fn from_str(s: &str) -> Result<Self, Self::Err> {
            <$type as $crate::FromFileString>::from_file_string(s)
        }
    }

    impl std::convert::TryFrom<&str> for $type {
        type Error = <$type as $crate::FromFileString>::Error;

        fn try_from(value: &str) -> Result<Self, Self::Error> {
            Ok(<$type as $crate::FromFileString>::from_file_string(value)?)
        }
    }

    impl std::convert::TryFrom<String> for $type {
        type Error = <$type as $crate::FromFileString>::Error;

        fn try_from(value: String) -> Result<Self, Self::Error> {
            Ok(<$type as $crate::FromFileString>::from_file_string(&value)?)
        }
    }

    impl std::convert::TryFrom<&String> for $type {
        type Error = <$type as $crate::FromFileString>::Error;

        fn try_from(value: &String) -> Result<Self, Self::Error> {
            Ok(<$type as $crate::FromFileString>::from_file_string(value)?)
        }
    }

    impl std::convert::TryFrom<&str> for Box<$type> {
        type Error = <$type as $crate::FromFileString>::Error;

        fn try_from(value: &str) -> Result<Self, Self::Error> {
            Ok(Box::new(<$type as $crate::FromFileString>::from_file_string(value)?))
        }
    }

    impl std::convert::TryFrom<String> for Box<$type> {
        type Error = <$type as $crate::FromFileString>::Error;

        fn try_from(value: String) -> Result<Self, Self::Error> {
            Ok(Box::new(<$type as $crate::FromFileString>::from_file_string(&value)?))
        }
    }

    impl std::convert::TryFrom<&String> for Box<$type> {
        type Error = <$type as $crate::FromFileString>::Error;

        fn try_from(value: &String) -> Result<Self, Self::Error> {
            Ok(Box::new(<$type as $crate::FromFileString>::from_file_string(value)?))
        }
    }
});

#[macro_export]
macro_rules! impl_file_string_serialization(($type:ty) => {
    impl serde::Serialize for $type {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where S: serde::Serializer {
            serializer.serialize_str(&$crate::ToFileString::to_file_string(self))
        }
    }

    impl<'de> serde::Deserialize<'de> for $type {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> where D: serde::Deserializer<'de> {
            let s = String::deserialize(deserializer)?;
            <$type as $crate::FromFileString>::from_file_string(&s).map_err(serde::de::Error::custom)
        }
    }
});

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case(Duration::ZERO, "0s")]
    #[case(Duration::from_millis(1), "1ms")]
    #[case(Duration::from_millis(1500), "1500ms")]
    #[case(Duration::from_secs(1), "1s")]
    #[case(Duration::from_secs(90), "90s")]
    #[case(Duration::from_secs(60), "1m")]
    #[case(Duration::from_secs(90 * 60), "90m")]
    #[case(Duration::from_secs(3 * 60 * 60), "3h")]
    #[case(Duration::from_secs(36 * 60 * 60), "36h")]
    #[case(Duration::from_secs(24 * 60 * 60), "1d")]
    #[case(Duration::from_secs(7 * 24 * 60 * 60), "7d")]
    #[case(Duration::from_secs(400 * 24 * 60 * 60), "400d")]
    #[case(Duration::from_nanos(1_500), "1500ns")]
    #[case(Duration::new(1, 1), "1000000001ns")]
    fn durations_should_use_a_canonical_file_string(#[case] duration: Duration, #[case] expected: &str) {
        assert_eq!(duration.to_file_string(), expected);
        assert_eq!(Duration::from_file_string(expected).unwrap(), duration);
    }

    #[rstest]
    #[case("0", "0s")]
    #[case("0ms", "0s")]
    #[case("3h", "3h")]
    #[case("180m", "3h")]
    #[case("1.5h", "90m")]
    #[case("1w", "7d")]
    #[case("86400s", "1d")]
    fn durations_should_normalize_equivalent_inputs(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(Duration::from_file_string(input).unwrap().to_file_string(), expected);
    }
}
