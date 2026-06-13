use clipanion::cli;
use zpm_primitives::Reference;
use zpm_utils::ToFileString;

use crate::error::Error;

/// Parse and print a package reference
///
/// This debug command prints the normalized string form and Rust debug representation of a reference.
///
#[cli::command(proxy)]
#[cli::path("debug", "check-reference")]
pub struct CheckReference {
    /// Reference to parse
    reference: Reference,
}

impl CheckReference {
    pub async fn execute(&self) -> Result<(), Error> {
        let stringified
            = self.reference.to_file_string();

        println!("{}", stringified);
        println!("{:#?}", self.reference);

        Ok(())
    }
}
