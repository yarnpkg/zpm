use clipanion::cli;
use zpm_primitives::Reference;
use zpm_utils::ToFileString;

use crate::error::Error;

#[cli::command(proxy)]
#[cli::path("debug", "check-reference")]
pub struct CheckReference {
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
