use clipanion::cli;
use zpm_primitives::Descriptor;
use zpm_utils::ToFileString;

use crate::{error::Error};

/// Parse and print a dependency descriptor
///
/// This debug command prints the normalized string form and Rust debug representation of a descriptor.
///
#[cli::command(proxy)]
#[cli::path("debug", "check-descriptor")]
pub struct CheckDescriptor {
    /// Descriptor to parse
    descriptor: Descriptor,
}

impl CheckDescriptor {
    pub async fn execute(&self) -> Result<(), Error> {
        let stringified
            = self.descriptor.to_file_string();

        println!("{}", stringified);
        println!("{:#?}", self.descriptor);

        Ok(())
    }
}
