use std::str::FromStr;

use clipanion::cli;

use crate::{error::Error, semver};

#[cli::command(proxy)]
#[cli::path("debug", "check-semver-version")]
pub struct CheckSemverVersion {
    version: String,
}

impl CheckSemverVersion {
    pub fn execute(&self) -> Result<(), Error> {
        let version = semver::Version::from_str(&self.version)?;
        let stringified = version.to_string();

        println!("{}", stringified);
        println!("{:#?}", version);

        Ok(())
    }
}
