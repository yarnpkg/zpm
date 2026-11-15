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

impl ToFileString for std::time::Duration {
    fn to_file_string(&self) -> String {
        let ms = self.as_millis();
        if ms == 0 {
            "0s".to_string()
        } else if ms % 1000 == 0 {
            format!("{}s", ms / 1000)
        } else {
            format!("{}ms", ms)
        }
    }
}

impl ToHumanString for std::time::Duration {
    fn to_print_string(&self) -> String {
        DataType::Number.colorize(&self.to_file_string())
    }
}

/// The unit in which a duration setting reads a value that has none (`1440`
/// rather than `1440m`); it's declared per setting in the configuration
/// schema, mirroring the `unit` of Berry's duration settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DurationUnit {
    Milliseconds,
    Seconds,
    Minutes,
    Hours,
    Days,
    Weeks,
}

impl DurationUnit {
    fn to_time_unit(self) -> fundu::TimeUnit {
        match self {
            DurationUnit::Milliseconds => fundu::TimeUnit::MilliSecond,
            DurationUnit::Seconds => fundu::TimeUnit::Second,
            DurationUnit::Minutes => fundu::TimeUnit::Minute,
            DurationUnit::Hours => fundu::TimeUnit::Hour,
            DurationUnit::Days => fundu::TimeUnit::Day,
            DurationUnit::Weeks => fundu::TimeUnit::Week,
        }
    }
}

/// Parses a duration, reading a value without unit as a number of `unit`.
/// The accepted units (`ms`, `s`, `m`, `h`, `d`, `w`, plus `ns` and `Ms` for
/// microseconds) mean the same thing as in Berry, and so do decimal values.
pub fn parse_duration_with_unit(s: &str, unit: DurationUnit) -> Result<std::time::Duration, fundu::ParseError> {
    let parser
        = fundu::DurationParser::builder()
            .default_time_units()
            .default_unit(unit.to_time_unit())
            .build();

    // Negative durations are rejected by the parser in this configuration, so
    // the conversion can't fail
    parser.parse(s)
        .map(|duration| duration.try_into().unwrap())
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

    use super::{parse_duration_with_unit, DurationUnit};

    #[rstest]
    // A value without unit is a number of the setting's unit
    #[case("1440", DurationUnit::Minutes, Duration::from_secs(24 * 60 * 60))]
    #[case("1440", DurationUnit::Seconds, Duration::from_secs(1440))]
    #[case("1440", DurationUnit::Milliseconds, Duration::from_millis(1440))]
    #[case("2", DurationUnit::Hours, Duration::from_secs(2 * 60 * 60))]
    #[case("2", DurationUnit::Days, Duration::from_secs(2 * 24 * 60 * 60))]
    #[case("2", DurationUnit::Weeks, Duration::from_secs(2 * 7 * 24 * 60 * 60))]
    #[case("0", DurationUnit::Minutes, Duration::ZERO)]
    // Decimals, with or without leading digit
    #[case("1.5", DurationUnit::Minutes, Duration::from_secs(90))]
    #[case(".5", DurationUnit::Minutes, Duration::from_secs(30))]
    #[case("0.25", DurationUnit::Seconds, Duration::from_millis(250))]
    #[case("1.5d", DurationUnit::Minutes, Duration::from_secs(36 * 60 * 60))]
    // An explicit unit wins over the setting's unit, whatever it is
    #[case("1440ms", DurationUnit::Minutes, Duration::from_millis(1440))]
    #[case("1440s", DurationUnit::Minutes, Duration::from_secs(1440))]
    #[case("30m", DurationUnit::Seconds, Duration::from_secs(30 * 60))]
    #[case("3h", DurationUnit::Milliseconds, Duration::from_secs(3 * 60 * 60))]
    #[case("7d", DurationUnit::Seconds, Duration::from_secs(7 * 24 * 60 * 60))]
    #[case("2w", DurationUnit::Minutes, Duration::from_secs(2 * 7 * 24 * 60 * 60))]
    #[case("0s", DurationUnit::Minutes, Duration::ZERO)]
    // Units Berry doesn't know about, kept for compatibility with earlier Yarn 6 releases
    #[case("1500Ms", DurationUnit::Minutes, Duration::from_micros(1500))]
    #[case("1500ns", DurationUnit::Minutes, Duration::from_nanos(1500))]
    fn test_parse_duration_with_unit(#[case] input: &str, #[case] unit: DurationUnit, #[case] expected: Duration) {
        assert_eq!(parse_duration_with_unit(input, unit).unwrap(), expected);
    }

    #[rstest]
    #[case("")]
    #[case("-5")]
    #[case("-5m")]
    #[case("5 m")]
    #[case("1M")]
    #[case("1y")]
    #[case("5 minutes")]
    #[case("abc")]
    fn test_parse_duration_with_unit_errors(#[case] input: &str) {
        assert!(parse_duration_with_unit(input, DurationUnit::Minutes).is_err());
    }
}
