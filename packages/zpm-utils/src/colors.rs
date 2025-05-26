use colored::{Color, Colorize};
use serde::{Deserialize, Deserializer};

use crate::{impl_serialization_traits_no_serde, FromFileString, ToFileString, ToHumanString};

const STRING_COLOR: Color
    = Color::TrueColor { r: 50, g: 170, b: 80 };

const NUMBER_COLOR: Color
    = Color::TrueColor { r: 255, g: 215, b: 0 };

const BOOLEAN_COLOR: Color
    = Color::TrueColor { r: 250, g: 160, b: 35 };

const NULL_COLOR: Color
    = Color::TrueColor { r: 160, g: 80, b: 180 };

const CODE_COLOR: Color
    = Color::TrueColor { r: 135, g: 175, b: 255 };

const PATH_COLOR: Color
    = Color::TrueColor { r: 215, g: 95, b: 215 };

const URL_COLOR: Color
    = Color::TrueColor { r: 215, g: 95, b: 215 };

pub enum DataType {
    String,
    Number,
    Boolean,
    Null,
    Code,
    Path,
    Url,
}

impl DataType {
    pub fn color(&self) -> Color {
        match self {
            DataType::String => STRING_COLOR,
            DataType::Number => NUMBER_COLOR,
            DataType::Boolean => BOOLEAN_COLOR,
            DataType::Null => NULL_COLOR,
            DataType::Code => CODE_COLOR,
            DataType::Path => PATH_COLOR,
            DataType::Url => URL_COLOR,
        }
    }

    pub fn colorize(&self, value: &str) -> String {
        value.color(self.color()).to_string()
    }
}
