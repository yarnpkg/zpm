use clipanion::cli;
use zpm_utils::{FromFileString, ToFileString};

use crate::{error::Error, primitives::Range};

#[cli::command(proxy)]
#[cli::path("debug", "check-range")]
pub struct CheckRange {
    range: String,
}

impl CheckRange {
    pub fn execute(&self) -> Result<(), Error> {
        let range = Range::from_file_string(&self.range)?;
        let stringified = range.to_file_string();

        println!("{}", stringified);
        println!("{:#?}", range);

        Ok(())
    }
}
