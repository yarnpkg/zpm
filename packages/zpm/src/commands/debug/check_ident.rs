use clipanion::cli;
use zpm_utils::{FromFileString, ToFileString};

use crate::{error::Error, primitives::Ident};

#[cli::command(proxy)]
#[cli::path("debug", "check-ident")]
pub struct CheckIdent {
    ident: String,
}

impl CheckIdent {
    pub fn execute(&self) -> Result<(), Error> {
        let ident = Ident::from_file_string(&self.ident)?;
        let stringified = ident.to_file_string();

        println!("{}", stringified);
        println!("{:#?}", ident);

        Ok(())
    }
}
