use std::{convert::Infallible, future::Future};

use crate::PathError;

pub fn is_default<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
}

/// Unwrapping an infallible result into its success value.
pub trait UnwrapInfallible {
    /// Type of the `Ok` variant of the result.
    type Ok;

    /// Unwraps a result, returning the content of an `Ok`.
    ///
    /// Unlike `Result::unwrap`, this method is known to never panic
    /// on the result types it is implemented for. Therefore, it can be used
    /// instead of `unwrap` as a maintainability safeguard that will fail
    /// to compile if the error type of the `Result` is later changed
    /// to an error that can actually occur.
    fn unwrap_infallible(self) -> Self::Ok;
}

impl<T> UnwrapInfallible for Result<T, Infallible> {
    type Ok = T;
    fn unwrap_infallible(self) -> T {
        self.unwrap_or_else(|never| match never {})
    }
}

pub trait IoResultExt<T, E> {
    fn discard_io_error(self, f: impl Fn(std::io::ErrorKind) -> bool) -> Result<Option<T>, E>;
    fn ok_missing(self) -> Result<Option<T>, E>;
    fn ok_exists(self) -> Result<Option<T>, E>;
}

impl<T> IoResultExt<T, std::io::Error> for Result<T, std::io::Error> {
    fn discard_io_error(self, f: impl Fn(std::io::ErrorKind) -> bool) -> Result<Option<T>, std::io::Error> {
        match self {
            Ok(value) => Ok(Some(value)),
            Err(err) if f(err.kind()) => Ok(None),
            Err(err) => Err(err),
        }
    }

    fn ok_missing(self) -> Result<Option<T>, std::io::Error> {
        self.discard_io_error(|kind| kind == std::io::ErrorKind::NotFound)
    }

    fn ok_exists(self) -> Result<Option<T>, std::io::Error> {
        self.discard_io_error(|kind| kind == std::io::ErrorKind::AlreadyExists)
    }
}

impl<T> IoResultExt<T, PathError> for Result<T, PathError> {
    fn discard_io_error(self, f: impl Fn(std::io::ErrorKind) -> bool) -> Result<Option<T>, PathError> {
        match self {
            Ok(value) => Ok(Some(value)),
            Err(err) if matches!(err.io_kind(), Some(kind) if f(kind)) => Ok(None),
            Err(err) => Err(err),
        }
    }

    fn ok_missing(self) -> Result<Option<T>, PathError> {
        self.discard_io_error(|kind| kind == std::io::ErrorKind::NotFound)
    }

    fn ok_exists(self) -> Result<Option<T>, PathError> {
        self.discard_io_error(|kind| kind == std::io::ErrorKind::AlreadyExists)
    }
}

pub fn diff_data(current: &[u8], expected: &[u8]) -> String {
    let current_text
        = String::from_utf8_lossy(current);
    let expected_text
        = String::from_utf8_lossy(expected);

    similar::TextDiff::from_lines(&current_text, &expected_text)
        .unified_diff()
        .to_string()
}

// Iterate over the values of the parameter; return the first result that succeeds, or the last error.
pub async fn repeat_until_ok<I, T, E, A, F>(values: Vec<I>, f: F) -> Result<T, E>
    where A: Future<Output = Result<T, E>>, F: Fn(I) -> A,
{
    let mut last_error = None;

    for value in values {
        let result
            = f(value).await;

        match result {
            Ok(value) => {
                return Ok(value);
            },

            Err(error) => {
                last_error = Some(error);
            },
        }
    }

    Err(last_error.unwrap())
}
