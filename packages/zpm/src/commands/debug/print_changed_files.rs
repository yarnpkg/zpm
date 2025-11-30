use clipanion::cli;
use zpm_utils::{Path, ToHumanString};

use crate::{error::Error, git_utils::fetch_changed_files};

#[cli::command]
#[cli::path("debug", "print-changed-files")]
pub struct PrintChangedFiles {
    #[cli::option("--root", default = Path::current_dir().unwrap())]
    root: Path,

    #[cli::option("--base", default = "HEAD".to_string())]
    base: String,
}

impl PrintChangedFiles {
    pub async fn execute(&self) -> Result<(), Error> {
        let changed_files
            = fetch_changed_files(&self.root, &self.base).await?;

        for file in changed_files {
            println!("{}", file.to_print_string());
        }

        Ok(())
    }
}
