use std::{fmt::Display, ops::{DivAssign, Rem}, sync::atomic::{AtomicBool, Ordering}};

use num::NumCast;
use serde::{Deserialize, Deserializer, Serialize};

use crate::{DataType, FromFileString, ToFileString, ToHumanString};

#[derive(Debug)]
pub struct UnitDefinition {
    initial: &'static str,
    units: &'static [(f64, &'static str)],
}

const BYTES: UnitDefinition = UnitDefinition {
    initial: " B",
    units: &[(1024.0, " KiB"), (1024.0, " MiB"), (1024.0, " GiB"), (1024.0, " TiB")],
};

const DURATION: UnitDefinition = UnitDefinition {
    initial: "s",
    units: &[(60.0, "m"), (60.0, "h"), (24.0, "d"), (7.0, "w"), (52.0, "y")],
};

#[derive(Debug)]
pub struct Unit<T> {
    pub value: T,
    pub unit_definition: &'static UnitDefinition,
}

impl<T> Unit<T> {
    pub fn bytes(value: T) -> Self {
        Self {value, unit_definition: &BYTES}
    }

    pub fn duration(value: T) -> Self {
        Self {value, unit_definition: &DURATION}
    }
}

#[derive(Debug, Clone)]
pub struct Secret<T> {
    pub value: T,
}

static REDACTED: AtomicBool = AtomicBool::new(true);

pub fn set_redacted(redacted: bool) {
    REDACTED.store(redacted, Ordering::Relaxed);
}

impl<T> Secret<T> {
    pub fn new(value: T) -> Self {
        Self {value}
    }
}

impl<T: FromFileString> FromFileString for Secret<T> {
    type Error = <T as FromFileString>::Error;

    fn from_file_string(s: &str) -> Result<Self, Self::Error> {
        Ok(Self {value: T::from_file_string(s)?})
    }
}

impl<T: ToFileString> ToFileString for Secret<T> {
    fn to_file_string(&self) -> String {
        if REDACTED.load(Ordering::Relaxed) {
            "<redacted>".to_string()
        } else {
            self.value.to_file_string()
        }
    }
}

impl<T: ToHumanString> ToHumanString for Secret<T> {
    fn to_print_string(&self) -> String {
        if REDACTED.load(Ordering::Relaxed) {
            DataType::Code.colorize("<redacted>")
        } else {
            self.value.to_print_string()
        }
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Secret<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> where D: Deserializer<'de> {
        Ok(Self {value: T::deserialize(deserializer)?})
    }
}

impl<T: Serialize> Serialize for Secret<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where S: serde::Serializer {
        if REDACTED.load(Ordering::Relaxed) {
            serializer.serialize_str("<redacted>")
        } else {
            self.value.serialize(serializer)
        }
    }
}

impl<T: DivAssign + Rem + PartialOrd + Display + Copy + NumCast> ToHumanString for Unit<T> {
    fn to_print_string(&self) -> String {
        let mut value: f64
            = NumCast::from(self.value).unwrap();

        let mut current_unit
            = self.unit_definition.initial;

        for (factor, unit) in self.unit_definition.units.iter().cloned() {
            if value < factor {
                break;
            }

            value /= factor;
            current_unit = unit;
        }

        DataType::Number.colorize(&format!("{:.2}{}", value, current_unit))
    }
}

impl<T: Serialize> Serialize for Unit<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where S: serde::Serializer {
        self.value.serialize(serializer)
    }
}

#[derive(Debug)]
pub struct TimeAgo {
    pub duration: std::time::Duration,
}

impl TimeAgo {
    pub fn new(duration: std::time::Duration) -> Self {
        Self {duration}
    }
}

impl ToHumanString for TimeAgo {
    fn to_print_string(&self) -> String {
        let f
            = timeago::Formatter::new();

        DataType::Number.colorize(&f.convert(self.duration))
    }
}

impl Serialize for TimeAgo {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where S: serde::Serializer {
        self.duration.serialize(serializer)
    }
}

#[derive(Debug)]
pub struct Serialized<T> {
    value: T,
}

impl<T> Serialized<T> {
    pub fn new(value: T) -> Self {
        Self {value}
    }
}

impl<T: Serialize> ToHumanString for Serialized<T> {
    fn to_print_string(&self) -> String {
        DataType::String.colorize(&serde_json::to_string(&self.value).unwrap())
    }
}

#[derive(Debug)]
pub struct RawString {
    value: String,
}

impl RawString {
    pub fn new(value: String) -> Self {
        Self {value}
    }
}

impl Serialize for RawString {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where S: serde::Serializer {
        serializer.serialize_str(&self.value)
    }
}

impl ToHumanString for RawString {
    fn to_print_string(&self) -> String {
        self.value.clone()
    }
}
