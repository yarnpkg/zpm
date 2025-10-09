use clipanion::cli;
use zpm_primitives::Ident;
use zpm_utils::ToFileString;

use crate::error::Error;

#[cli::command(proxy)]
#[cli::path("debug", "check-ident")]
pub struct CheckIdent {
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
