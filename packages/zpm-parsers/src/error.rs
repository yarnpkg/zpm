#[derive(thiserror::Error, Clone, Debug)]
pub enum Error {
    #[error("Utf8 conversion error: {0}")]
    Utf8Conversion(#[from] std::str::Utf8Error),

    #[error("Invalid UTF-8 sequence: {0}")]
    InvalidUtf8Sequence(#[from] std::string::FromUtf8Error),

    #[error("Invalid syntax: {0}")]
    InvalidSyntax(String),

    #[error("Invalid array access: {0}")]
    InvalidArrayAccess(String),

    #[error("Cannot navigate through primitive value")]
    InvalidPrimitiveNavigation,
}

impl From<sonic_rs::Error> for Error {
    fn from(error: sonic_rs::Error) -> Self {
        Error::InvalidSyntax(error.to_string())
    }
}
