use clipanion::cli;
use zpm_utils::{Path, ToFileString};

use crate::{error::Error};

/// List entries in a zip archive
///
/// This debug command prints each entry path contained in the specified zip file.
///
#[cli::command(proxy)]
#[cli::path("debug", "iter-zip")]
pub struct IterZip {
    /// Zip archive to inspect
    path: Path,
}

impl IterZip {
    pub async fn execute(&self) -> Result<(), Error> {
        let buffer = self.path
            .fs_read()?;

        let entries
            = zpm_formats::zip::entries_from_zip(&buffer)?;

        for entry in entries {
            println!("{}", entry.name.to_file_string());
        }

        Ok(())
    }
}
