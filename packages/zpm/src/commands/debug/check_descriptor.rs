use clipanion::cli;
use zpm_primitives::Descriptor;
use zpm_utils::ToFileString;

use crate::{error::Error};

#[cli::command(proxy)]
#[cli::path("debug", "check-descriptor")]
pub struct CheckDescriptor {
    descriptor: Descriptor,
}

impl CheckDescriptor {
    pub fn execute(&self) -> Result<(), Error> {
        let stringified
            = self.descriptor.to_file_string();

        println!("{}", stringified);
        println!("{:#?}", self.descriptor);

        Ok(())
    }
}
