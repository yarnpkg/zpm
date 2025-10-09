use std::fmt;

use colored::Colorize;
use erased_serde::serialize_trait_object;
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
}

impl<'a> AbstractValue<'a> {
    pub fn new<T: Extracted + 'a>(value: T) -> Self {
        Self {value: Box::new(value)}
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
    type Error = std::num::ParseIntError;

    fn from_file_string(s: &str) -> Result<Self, Self::Error> {
        s.parse::<u64>().map(|s| std::time::Duration::from_secs(s as u64))
    }
}

impl ToFileString for std::time::Duration {
    fn to_file_string(&self) -> String {
        self.as_secs().to_string()
    }
}

impl ToHumanString for std::time::Duration {
    fn to_print_string(&self) -> String {
        DataType::Number.colorize(&self.to_file_string())
    }
}

impl FromFileString for String {
    type Error = std::convert::Infallible;

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
        "null".to_string()
    }
}

impl<T: ToFileString> ToHumanString for Option<T> {
    fn to_print_string(&self) -> String {
        self.to_file_string()
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
            use $crate::ToFileString;
            serializer.serialize_str(&self.to_file_string())
        }
    }

    impl<'de> serde::Deserialize<'de> for $type {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> where D: serde::Deserializer<'de> {
            let s = String::deserialize(deserializer)?;
            <$type as $crate::FromFileString>::from_file_string(&s).map_err(serde::de::Error::custom)
        }
    }
});
