use std::{borrow::Cow, convert::Infallible, future::Future};

use crate::PathError;

pub fn is_default<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
}

/**
 * Turns CRLF sequences (and lone CR) into LF.
 *
 * Text whose hash ends up inside a locator has to go through this first:
 * the same file checked out on Windows (`core.autocrlf` is on by default
 * there) would otherwise hash differently than on Unix, and the lockfile
 * would flip-flop between contributors without the installed packages
 * changing at all.
 */
pub fn normalize_line_endings(data: &[u8]) -> Cow<'_, [u8]> {
    if !data.contains(&b'\r') {
        return Cow::Borrowed(data);
    }

    let mut normalized
        = Vec::with_capacity(data.len());

    let mut index = 0;
    while index < data.len() {
        if data[index] == b'\r' {
            normalized.push(b'\n');

            // A CRLF pair only stands for a single line break
            if data.get(index + 1) == Some(&b'\n') {
                index += 1;
            }
        } else {
            normalized.push(data[index]);
        }

        index += 1;
    }

    Cow::Owned(normalized)
}

pub trait UnwrapInfallible<T> {
    fn unwrap_infallible(self) -> T;
}

impl<T> UnwrapInfallible<T> for Result<T, Infallible> {
    fn unwrap_infallible(self) -> T {
        self.unwrap()
    }
}

pub trait ResultExt<T, E> {
    fn discard_error(self, f: impl Fn(&E) -> bool) -> Result<Option<T>, E>;
}

impl<T, E> ResultExt<T, E> for Result<T, E> {
    fn discard_error(self, f: impl Fn(&E) -> bool) -> Result<Option<T>, E> {
        match self {
            Ok(value) => Ok(Some(value)),
            Err(err) if f(&err) => Ok(None),
            Err(err) => Err(err),
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_line_endings_borrows_lf_only_text() {
        let normalized
            = normalize_line_endings(b"one\ntwo\n");

        assert!(matches!(normalized, Cow::Borrowed(_)));
        assert_eq!(normalized.as_ref(), b"one\ntwo\n");
    }

    #[test]
    fn normalize_line_endings_rewrites_crlf_and_lone_cr() {
        assert_eq!(normalize_line_endings(b"one\r\ntwo\r\n").as_ref(), b"one\ntwo\n");
        assert_eq!(normalize_line_endings(b"one\rtwo\r").as_ref(), b"one\ntwo\n");
        assert_eq!(normalize_line_endings(b"one\r\ntwo\nthree\r").as_ref(), b"one\ntwo\nthree\n");
    }

    #[test]
    fn normalize_line_endings_keeps_the_rest_of_the_content() {
        assert_eq!(normalize_line_endings(b"").as_ref(), b"");
        assert_eq!(normalize_line_endings(b"no newline").as_ref(), b"no newline");
        assert_eq!(normalize_line_endings(&[0xff, b'\r', b'\n', 0x00]).as_ref(), &[0xff, b'\n', 0x00]);
    }
}
