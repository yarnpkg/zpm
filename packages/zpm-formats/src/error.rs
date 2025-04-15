use std::sync::Arc;

use zpm_utils::PathError;

#[derive(thiserror::Error, Clone, Debug)]
pub enum Error {
    #[error("Invalid zip file: {0}")]
    InvalidZipFile(String),

    #[error("{0}")]
    PathError(#[from] PathError),

    #[error("I/O error: {0}")]
    Io(#[from] Arc<std::io::Error>),

    #[error("Utf8 conversion error: {0}")]
    Utf8Conversion(#[from] std::str::Utf8Error),

    #[error("Invalid tar file path: {0}")]
    InvalidTarFilePath(String),
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(Arc::new(e))
    }
}
