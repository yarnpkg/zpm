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

    #[error("Using 'mixed' as compression level is deprecated - the compression will now be automatically mixed if beneficial")]
    MixedValueDeprecated,

    #[error("Compression level must be between 0 and 9")]
    InvalidCompressionLevel,

    #[error("Invalid os string conversion")]
    OsStringConversion(std::ffi::OsString),

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

impl From<std::ffi::OsString> for Error {
    fn from(e: std::ffi::OsString) -> Self {
        Error::OsStringConversion(e)
    }
}
