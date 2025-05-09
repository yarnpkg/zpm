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
