use std::str::FromStr;

use clipanion::cli;

use crate::{error::Error, primitives::Range};

#[cli::command(proxy)]
#[cli::path("debug", "check-range")]
pub struct CheckRange {
    range: String,
}

impl CheckRange {
    pub fn execute(&self) -> Result<(), Error> {
        let range = Range::from_str(&self.range)?;
        let stringified = range.to_string();

        println!("{}", stringified);
        println!("{:#?}", range);

        Ok(())
    }
}
