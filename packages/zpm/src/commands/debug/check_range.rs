use clipanion::cli;
use zpm_primitives::Range;
use zpm_utils::ToFileString;

use crate::error::Error;

#[cli::command(proxy)]
#[cli::path("debug", "check-range")]
pub struct CheckRange {
    range: Range,
}

impl CheckRange {
    pub async fn execute(&self) -> Result<(), Error> {
        let stringified
            = self.range.to_file_string();

        println!("{}", stringified);
        println!("{:#?}", self.range);

        Ok(())
    }
}
