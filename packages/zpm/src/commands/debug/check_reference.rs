use std::str::FromStr;

use clipanion::cli;

use crate::{error::Error, primitives::Reference};

#[cli::command(proxy)]
#[cli::path("debug", "check-reference")]
pub struct CheckReference {
    reference: String,
}

impl CheckReference {
    pub fn execute(&self) -> Result<(), Error> {
        let reference = Reference::from_str(&self.reference)?;
        let stringified = reference.to_string();

        println!("{}", stringified);
        println!("{:#?}", reference);

        Ok(())
    }
}
