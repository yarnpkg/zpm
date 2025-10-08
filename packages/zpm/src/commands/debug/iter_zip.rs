use clipanion::cli;
use zpm_utils::Path;

use crate::{error::Error};

#[cli::command(proxy)]
#[cli::path("debug", "iter-zip")]
pub struct IterZip {
    path: Path,
}

impl IterZip {
    pub async fn execute(&self) -> Result<(), Error> {
        let buffer = self.path
            .fs_read()?;

        let entries
            = zpm_formats::zip::entries_from_zip(&buffer)?;

        for entry in entries {
            println!("{}", entry.name);
        }

        Ok(())
    }
}
