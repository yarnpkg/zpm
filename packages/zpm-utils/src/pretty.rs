use std::{fmt::Display, ops::{DivAssign, Rem}};

use num::NumCast;
use serde::Serialize;

use crate::{DataType, ToHumanString};

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
