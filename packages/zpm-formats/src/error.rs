use std::sync::Arc;

use zpm_utils::PathError;

#[derive(thiserror::Error, Clone, Debug)]
pub enum Error {
    #[error(transparent)]
    PathError(#[from] PathError),

    #[error(transparent)]
    Io(#[from] Arc<std::io::Error>),

    #[error(transparent)]
    Utf8Conversion(#[from] std::str::Utf8Error),

    #[error("Invalid zip file")]
    InvalidZipFile(String),

    #[error("Invalid tar file path: {0}")]
    InvalidTarFilePath(String),

    #[error("Invalid tar file")]
    InvalidTarFile,
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(Arc::new(e))
    }
}
