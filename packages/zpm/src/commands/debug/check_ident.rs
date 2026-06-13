use clipanion::cli;
use zpm_primitives::Ident;
use zpm_utils::ToFileString;

use crate::error::Error;

/// Parse and print a package ident
///
/// This debug command prints the normalized string form and Rust debug representation of an ident.
///
#[cli::command(proxy)]
#[cli::path("debug", "check-ident")]
pub struct CheckIdent {
    /// Ident to parse
    ident: Ident,
}

impl CheckIdent {
    pub async fn execute(&self) -> Result<(), Error> {
        let stringified
            = self.ident.to_file_string();

        println!("{}", stringified);
        println!("{:#?}", self.ident);

        Ok(())
    }
}
