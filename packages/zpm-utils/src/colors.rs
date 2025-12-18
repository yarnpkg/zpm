use colored::{Color, Colorize};

const INFO_COLOR: Color
    = Color::TrueColor { r: 87, g: 163, b: 255 };

const WARNING_COLOR: Color
    = Color::TrueColor { r: 255, g: 87, b: 51 };

const ERROR_COLOR: Color
    = Color::TrueColor { r: 200, g: 100, b: 100 };

const SUCCESS_COLOR: Color
    = Color::BrightGreen;

const STRING_COLOR: Color
    = Color::TrueColor { r: 50, g: 170, b: 80 };

const NUMBER_COLOR: Color
    = Color::TrueColor { r: 255, g: 215, b: 0 };

const SIZE_COLOR: Color
    = Color::TrueColor { r: 120, g: 100, b: 200 };

const DURATION_COLOR: Color
    = Color::TrueColor { r: 180, g: 180, b: 180 };

const BOOLEAN_COLOR: Color
    = Color::TrueColor { r: 250, g: 160, b: 35 };

const NULL_COLOR: Color
    = Color::TrueColor { r: 160, g: 80, b: 180 };

const CODE_COLOR: Color
    = Color::TrueColor { r: 135, g: 175, b: 255 };

const PATH_COLOR: Color
    = Color::TrueColor { r: 215, g: 130, b: 215 };

const URL_COLOR: Color
    = Color::TrueColor { r: 215, g: 130, b: 215 };

const IDENT_COLOR: Color
    = Color::TrueColor { r: 215, g: 135, b: 95 };

const RANGE_COLOR: Color
    = Color::TrueColor { r: 0, g: 175, b: 175 };

const REFERENCE_COLOR: Color
    = Color::TrueColor { r: 135, g: 175, b: 255 };

#[derive(Debug, Clone, Copy)]
pub enum DataType {
    Info,
    Warning,
    Error,
    Success,
    String,
    Number,
    Size,
    Duration,
    Boolean,
    Null,
    Code,
    Path,
    Url,
    Ident,
    Range,
    Reference,
    Custom(u8, u8, u8),
}

impl DataType {
    pub fn color(&self) -> Color {
        match self {
            DataType::Info => INFO_COLOR,
            DataType::Warning => WARNING_COLOR,
            DataType::Error => ERROR_COLOR,
            DataType::Success => SUCCESS_COLOR,
            DataType::String => STRING_COLOR,
            DataType::Number => NUMBER_COLOR,
            DataType::Size => SIZE_COLOR,
            DataType::Duration => DURATION_COLOR,
            DataType::Boolean => BOOLEAN_COLOR,
            DataType::Null => NULL_COLOR,
            DataType::Code => CODE_COLOR,
            DataType::Path => PATH_COLOR,
            DataType::Url => URL_COLOR,
            DataType::Ident => IDENT_COLOR,
            DataType::Range => RANGE_COLOR,
            DataType::Reference => REFERENCE_COLOR,
            DataType::Custom(r, g, b) => Color::TrueColor {r: *r, g: *g, b: *b},
        }
    }

    pub fn colorize(&self, value: &str) -> String {
        value.color(self.color()).to_string()
    }
}
