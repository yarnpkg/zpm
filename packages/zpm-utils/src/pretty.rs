use std::{fmt::Display, ops::{DivAssign, Rem}};

use num::{Integer, NumCast};
use serde::Serialize;

use crate::{DataType, ToHumanString};

const BYTE_UNITS: [&str; 4] = ["B", "KiB", "MiB", "GiB"];
const FINAL_UNIT: &str = "TiB";

#[derive(Debug)]
pub struct Size<T> {
    pub size: T,
}

impl<T> Size<T> {
    pub fn new(size: T) -> Self {
        Self {size}
    }
}

impl<T: Integer + DivAssign + Rem + PartialOrd + Display + Copy + NumCast> ToHumanString for Size<T> {
    fn to_print_string(&self) -> String {
        let mut value: f64
            = NumCast::from(self.size).unwrap();

        for unit in BYTE_UNITS.iter() {
            if value < 1024.0 {
                return DataType::Number.colorize(&format!("{:.2} {}", value, unit));
            }

            value /= 1024.0;
        }

        DataType::Number.colorize(&format!("{:.2} {}", value, FINAL_UNIT))
    }
}

impl<T: Serialize> Serialize for Size<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where S: serde::Serializer {
        self.size.serialize(serializer)
    }
}
