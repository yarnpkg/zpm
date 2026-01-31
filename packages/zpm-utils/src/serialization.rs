use std::{fmt, string::FromUtf8Error};

use colored::Colorize;
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
    fn write_file_string<W: fmt::Write>(&self, out: &mut W) -> fmt::Result;

    fn to_file_string(&self) -> String {
        format!("{}", FileStringDisplay(self))
    }
}

pub struct FileStringDisplay<'a, T: ?Sized>(pub &'a T);

impl<'a, T: ToFileString + ?Sized> fmt::Display for FileStringDisplay<'a, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.write_file_string(f)
    }
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
    fn write_file_string<W: fmt::Write>(&self, out: &mut W) -> fmt::Result {
        self.as_ref().write_file_string(out)
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
    fn write_file_string<W: fmt::Write>(&self, out: &mut W) -> fmt::Result {
        write!(out, "{}", self)
    }
}

impl ToHumanString for bool {
    fn to_print_string(&self) -> String {
        let mut buffer = String::new();
        let _ = self.write_file_string(&mut buffer);
        DataType::Boolean.colorize(&buffer)
    }
}

impl FromFileString for usize {
    type Error = std::num::ParseIntError;

    fn from_file_string(s: &str) -> Result<Self, Self::Error> {
        s.parse()
    }
}

impl ToFileString for usize {
    fn write_file_string<W: fmt::Write>(&self, out: &mut W) -> fmt::Result {
        write!(out, "{}", self)
    }
}

impl ToHumanString for usize {
    fn to_print_string(&self) -> String {
        let mut buffer = String::new();
        let _ = self.write_file_string(&mut buffer);
        DataType::Number.colorize(&buffer)
    }
}

impl FromFileString for u64 {
    type Error = std::num::ParseIntError;

    fn from_file_string(s: &str) -> Result<Self, Self::Error> {
        s.parse()
    }
}

impl ToFileString for u64 {
    fn write_file_string<W: fmt::Write>(&self, out: &mut W) -> fmt::Result {
        write!(out, "{}", self)
    }
}

impl ToHumanString for u64 {
    fn to_print_string(&self) -> String {
        let mut buffer = String::new();
        let _ = self.write_file_string(&mut buffer);
        DataType::Number.colorize(&buffer)
    }
}

impl FromFileString for std::time::Duration {
    type Error = fundu::ParseError;

    fn from_file_string(s: &str) -> Result<Self, Self::Error> {
        parse_duration(s)
    }
}

impl ToFileString for std::time::Duration {
    fn write_file_string<W: fmt::Write>(&self, out: &mut W) -> fmt::Result {
        write!(out, "{}", self.as_secs())
    }
}

impl ToHumanString for std::time::Duration {
    fn to_print_string(&self) -> String {
        let mut buffer = String::new();
        let _ = self.write_file_string(&mut buffer);
        DataType::Number.colorize(&buffer)
    }
}

impl FromFileString for String {
    type Error = FromUtf8Error;

    fn from_file_string(s: &str) -> Result<Self, Self::Error> {
        Ok(s.to_string())
    }
}

impl ToFileString for String {
    fn write_file_string<W: fmt::Write>(&self, out: &mut W) -> fmt::Result {
        out.write_str(self)
    }
}

impl ToHumanString for String {
    fn to_print_string(&self) -> String {
        let mut buffer = String::new();
        let _ = self.write_file_string(&mut buffer);
        DataType::String.colorize(&buffer)
    }
}

impl ToFileString for &str {
    fn write_file_string<W: fmt::Write>(&self, out: &mut W) -> fmt::Result {
        out.write_str(self)
    }
}

impl ToHumanString for &str {
    fn to_print_string(&self) -> String {
        let mut buffer = String::new();
        let _ = self.write_file_string(&mut buffer);
        buffer.truecolor(0, 153, 0).to_string()
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
    fn write_file_string<W: fmt::Write>(&self, out: &mut W) -> fmt::Result {
        if let Some(value) = self {
            value.write_file_string(out)
        } else {
            out.write_str("null")
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
            serializer.collect_str(&$crate::FileStringDisplay(self))
        }
    }

    impl<'de> serde::Deserialize<'de> for $type {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> where D: serde::Deserializer<'de> {
            let s = String::deserialize(deserializer)?;
            <$type as $crate::FromFileString>::from_file_string(&s).map_err(serde::de::Error::custom)
        }
    }
});
