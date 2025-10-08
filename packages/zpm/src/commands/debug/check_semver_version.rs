use clipanion::cli;
use zpm_utils::{FromFileString, ToFileString};

use crate::error::Error;

#[cli::command(proxy)]
#[cli::path("debug", "check-semver-version")]
pub struct CheckSemverVersion {
    version: String,
}

impl CheckSemverVersion {
    pub async fn execute(&self) -> Result<(), Error> {
        let version
            = zpm_semver::Version::from_file_string(&self.version)?;
        let stringified
            = version.to_file_string();

        println!("{}", stringified);
        println!("{:#?}", version);

        Ok(())
    }
}
