use clipanion::cli;
use zpm_utils::{FromFileString, ToFileString};

use crate::{error::Error, primitives::Descriptor};

#[cli::command(proxy)]
#[cli::path("debug", "check-descriptor")]
pub struct CheckDescriptor {
    descriptor: String,
}

impl CheckDescriptor {
    pub fn execute(&self) -> Result<(), Error> {
        let descriptor
            = Descriptor::from_file_string(&self.descriptor)?;
        let stringified
            = descriptor.to_file_string();

        println!("{}", stringified);
        println!("{:#?}", descriptor);

        Ok(())
    }
}
