#[derive(thiserror::Error, Clone, Debug)]
pub enum Error {
    #[error("Utf8 conversion error: {0}")]
    Utf8Conversion(#[from] std::str::Utf8Error),
}
