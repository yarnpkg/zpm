use clipanion::cli;
use zpm_primitives::Range;
use zpm_utils::ToFileString;

use crate::error::Error;

/// Parse and print a dependency range
///
/// This debug command prints the normalized string form and Rust debug representation of a range.
///
#[cli::command(proxy)]
#[cli::path("debug", "check-range")]
pub struct CheckRange {
    /// Range to parse
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
