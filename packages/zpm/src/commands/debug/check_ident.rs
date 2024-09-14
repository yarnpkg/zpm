use std::str::FromStr;

use clipanion::cli;

use crate::{error::Error, primitives::Ident};

#[cli::command(proxy)]
#[cli::path("debug", "check-ident")]
pub struct CheckIdent {
    ident: String,
}

impl CheckIdent {
    pub fn execute(&self) -> Result<(), Error> {
        let ident = Ident::from_str(&self.ident)?;
        let stringified = ident.to_string();

        println!("{}", stringified);
        println!("{:#?}", ident);

        Ok(())
    }
}
