use clipanion::cli;
use zpm_primitives::Locator;
use zpm_utils::ToFileString;

use crate::{error::Error};

/// Parse and print a package locator
///
/// This debug command prints the normalized string form and Rust debug representation of a locator.
///
#[cli::command(proxy)]
#[cli::path("debug", "check-locator")]
pub struct CheckLocator {
    /// Locator to parse
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
