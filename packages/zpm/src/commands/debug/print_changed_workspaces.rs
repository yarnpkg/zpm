use clipanion::cli;
use zpm_utils::ToHumanString;

use crate::{error::Error, git_utils::fetch_changed_workspaces, project};

#[cli::command]
#[cli::path("debug", "print-changed-workspaces")]
pub struct PrintChangedWorkspaces {
    #[cli::option("--base", default = "HEAD".to_string())]
    base: String,
}

impl PrintChangedWorkspaces {
    pub async fn execute(&self) -> Result<(), Error> {
        let project
            = project::Project::new(None).await?;

        let changed_workspaces
            = fetch_changed_workspaces(&project, &self.base).await?;

        for ident in changed_workspaces.keys() {
            println!("{}", ident.to_print_string());
        }

        Ok(())
    }
}
