use clipanion::cli;

use crate::{error::Error, git_utils::fetch_branch_base, project};

#[cli::command]
#[cli::path("debug", "print-branch-base")]
pub struct PrintBranchBase {
}

impl PrintBranchBase {
    pub async fn execute(&self) -> Result<(), Error> {
        let project
            = project::Project::new(None).await?;

        let branch_base
            = fetch_branch_base(&project).await?;

        println!("{}", branch_base);

        Ok(())
    }
}
