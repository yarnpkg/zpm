use crate::PathError;

pub fn is_default<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
}

pub trait OkMissing<T, E> {
    fn ok_missing(self) -> Result<Option<T>, E>;
}

impl<T> OkMissing<T, std::io::Error> for Result<T, std::io::Error> {
    fn ok_missing(self) -> Result<Option<T>, std::io::Error> {
        match self {
            Ok(value) => Ok(Some(value)),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(err),
        }
    }
}

impl<T> OkMissing<T, PathError> for Result<T, PathError> {
    fn ok_missing(self) -> Result<Option<T>, PathError> {
        match self {
            Ok(value) => Ok(Some(value)),
            Err(err) if err.io_kind() == Some(std::io::ErrorKind::NotFound) => Ok(None),
            Err(err) => Err(err),
        }
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
