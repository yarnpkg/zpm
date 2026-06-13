use clipanion::cli;
use zpm_utils::ToHumanString;

use crate::{error::Error, git_utils::fetch_changed_workspaces, project};

/// Print workspaces changed since a git ref
#[cli::command]
#[cli::path("debug", "print-changed-workspaces")]
pub struct PrintChangedWorkspaces {
    /// Ref to compare against; defaults to the configured branch base
    #[cli::option("--since")]
    since: Option<String>,
}

impl PrintChangedWorkspaces {
    pub async fn execute(&self) -> Result<(), Error> {
        let project
            = project::Project::new(None).await?;

        let changed_workspaces
            = fetch_changed_workspaces(&project, self.since.as_deref()).await?;

        for ident in changed_workspaces.keys() {
            println!("{}", ident.to_print_string());
        }

        Ok(())
    }
}
