use std::sync::Arc;

use thiserror::Error;

#[derive(Error, Clone, Debug)]
pub enum PathError {
    #[error("Immutable paths cannot be modified")]
    Immutable,

    #[error("I/O error: {0}")]
    Io(#[from] Arc<std::io::Error>),

    #[error("UTF-8 path error: {0}")]
    FromUtf8Error(#[from] std::str::Utf8Error),
}

impl From<std::io::Error> for PathError {
    fn from(e: std::io::Error) -> Self {
        PathError::Io(Arc::new(e))
    }
}
