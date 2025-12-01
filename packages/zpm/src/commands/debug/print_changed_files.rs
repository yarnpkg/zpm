use clipanion::cli;
use zpm_utils::ToHumanString;

use crate::{error::Error, git_utils::fetch_changed_files, project};

#[cli::command]
#[cli::path("debug", "print-changed-files")]
pub struct PrintChangedFiles {
    #[cli::option("--since")]
    since: Option<String>,
}

impl PrintChangedFiles {
    pub async fn execute(&self) -> Result<(), Error> {
        let project
            = project::Project::new(None).await?;

        let changed_files
            = fetch_changed_files(&project, self.since.as_deref()).await?;

        for file in changed_files {
            println!("{}", file.to_print_string());
        }

        Ok(())
    }
}
