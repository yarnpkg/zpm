use std::sync::{atomic::{AtomicUsize, Ordering}, Arc};

use thiserror::Error;

use crate::{Path, ToHumanString};

static CAPTURED_BACKTRACES: AtomicUsize
    = AtomicUsize::new(0);

/// Captures the backtrace attached to I/O errors. When `RUST_BACKTRACE` is
/// set, each capture walks the whole stack, which is expensive (very much so
/// in static musl builds), so code probing the filesystem in loops should
/// work on `std::io::Result` and only convert the errors it reports.
pub fn capture_backtrace() -> std::backtrace::Backtrace {
    let backtrace
        = std::backtrace::Backtrace::capture();

    if backtrace.status() == std::backtrace::BacktraceStatus::Captured {
        CAPTURED_BACKTRACES.fetch_add(1, Ordering::Relaxed);
    }

    backtrace
}

/// How many backtraces `capture_backtrace` collected since the process
/// started; used by the tests to detect expected errors on hot paths.
pub fn captured_backtrace_count() -> usize {
    CAPTURED_BACKTRACES.load(Ordering::Relaxed)
}

pub fn render_backtrace(backtrace: &std::backtrace::Backtrace) -> String {
    if backtrace.status() == std::backtrace::BacktraceStatus::Captured {
        backtrace.to_string().trim_end().to_string()
    } else {
        "Run with RUST_BACKTRACE=1 to get a backtrace".to_string()
    }
}

#[derive(Error, Clone, Debug)]
pub enum EnumError {
    #[error("Enum not found: {0}")]
    NotFound(String),
}

#[derive(Error, Clone, Debug)]
pub enum PathError {
    #[error("Immutable paths cannot be modified (when modifying {path}; current mode: {current_mode:?}, expected mode: {expected_mode:?})", path = path.to_print_string())]
    ImmutablePermissions {
        path: Path,
        current_mode: u32,
        expected_mode: u32,
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

    #[error("Atomic rename conflict while moving {from} to {to} ({inner})", from = from.to_print_string(), to = to.to_print_string())]
    AtomicRenameConflict {
        from: Path,
        to: Path,
        inner: Arc<PathError>,
    },

    #[error("Invalid UTF8 path")]
    InvalidUtf8Path,

    #[error("Invalid explicit path parameter: {0}")]
    InvalidExplicitPathParameter(String),
}

impl PathError {
    pub fn io_kind(&self) -> Option<std::io::ErrorKind> {
        match self {
            PathError::IoError {inner, ..} => {
                Some(inner.kind())
            },

            PathError::AtomicRenameConflict {inner, ..} => {
                inner.io_kind()
            },

            _ => {
                None
            },
        }
    }
}

impl From<std::io::Error> for PathError {
    fn from(error: std::io::Error) -> Self {
        Self::IoError {
            inner: Arc::new(error),
            backtrace: Arc::new(capture_backtrace()),
        }
    }
}

impl From<std::str::Utf8Error> for PathError {
    fn from(_: std::str::Utf8Error) -> Self {
        Self::InvalidUtf8Path
    }
}
