use std::str::FromStr;

use zpm_utils::RawPath;

use crate::errors::Error;

#[derive(Debug)]
pub struct ExplicitPath {
    pub raw_path: RawPath,
}

impl FromStr for ExplicitPath {
    type Err = Error;

    fn from_str(val: &str) -> Result<ExplicitPath, Error> {
        if !val.contains('/') {
            return Err(Error::InvalidExplicitPathParameter);
        }

        let raw_path
            = RawPath::try_from(val)?;

        Ok(ExplicitPath {
            raw_path,
        })
    }
}
