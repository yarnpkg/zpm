use std::sync::Arc;

use thiserror::Error;

use crate::{Path, ToHumanString};

pub fn render_backtrace(backtrace: &std::backtrace::Backtrace) -> String {
    if backtrace.status() == std::backtrace::BacktraceStatus::Captured {
        backtrace.to_string().trim_end().to_string()
    } else {
        "Run with RUST_BACKTRACE=1 to get a backtrace".to_string()
    }
}

#[derive(Error, Clone, Debug)]
pub enum PathError {
    #[error("Immutable paths cannot be modified (when modifying {path}; current permissions: {current_permissions:?}, expected permissions: {expected_permissions:?})", path = path.to_print_string())]
    ImmutablePermissions {
        path: Path,
        current_permissions: std::fs::Permissions,
        expected_permissions: std::fs::Permissions,
    },

    #[error("Immutable paths cannot be modified (when modifying {path}); diff:\n{diff}", path = path.to_print_string(), diff = diff.as_ref().map(|diff| diff.as_str()).unwrap_or(""))]
    ImmutableData {
        path: Path,
        diff: Option<String>,
    },

    #[error("I/O error ({inner})\n\n{}", render_backtrace(backtrace))]
    IoError {
        inner: Arc<std::io::Error>,
        backtrace: Arc<std::backtrace::Backtrace>,
    },

    #[error("UTF-8 path error: {0}")]
    FromUtf8Error(#[from] std::str::Utf8Error),

    #[error("Invalid explicit path parameter: {0}")]
    InvalidExplicitPathParameter(String),
}

impl PathError {
    pub fn io_kind(&self) -> Option<std::io::ErrorKind> {
        if let PathError::IoError {inner, ..} = self {
            Some(inner.kind())
        } else {
            None
        }
    }
}

impl From<std::io::Error> for PathError {
    fn from(error: std::io::Error) -> Self {
        Self::IoError {
            inner: Arc::new(error),
            backtrace: Arc::new(std::backtrace::Backtrace::capture()),
        }
    }
}
