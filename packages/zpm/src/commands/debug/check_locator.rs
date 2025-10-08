use clipanion::cli;
use zpm_primitives::Locator;
use zpm_utils::ToFileString;

use crate::{error::Error};

#[cli::command(proxy)]
#[cli::path("debug", "check-locator")]
pub struct CheckLocator {
    locator: Locator,
}

impl CheckLocator {
    pub async fn execute(&self) -> Result<(), Error> {
        let stringified
            = self.locator.to_file_string();

        println!("{}", stringified);
        println!("{:#?}", self.locator);

        Ok(())
    }
}
