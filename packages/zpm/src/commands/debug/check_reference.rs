use clipanion::cli;
use zpm_utils::{FromFileString, ToFileString};

use crate::{error::Error, primitives::Reference};

#[cli::command(proxy)]
#[cli::path("debug", "check-reference")]
pub struct CheckReference {
    reference: String,
}

impl CheckReference {
    pub fn execute(&self) -> Result<(), Error> {
        let reference = Reference::from_file_string(&self.reference)?;
        let stringified = reference.to_file_string();

        println!("{}", stringified);
        println!("{:#?}", reference);

        Ok(())
    }
}
