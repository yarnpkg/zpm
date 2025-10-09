use std::process::ExitStatus;

use clipanion::cli;

use crate::{error::Error, project, script::ScriptEnvironment};

/// Run a Node.js script in the package environment
#[cli::command(proxy)]
#[cli::path("node")]
#[cli::category("Scripting commands")]
pub struct Node {
    args: Vec<String>,
}

impl Node {
    pub async fn execute(&self) -> Result<ExitStatus, Error> {
        let project
            = project::Project::new(None).await?;

        Ok(ScriptEnvironment::new()?
            .with_project(&project)
            .enable_shell_forwarding()
            .run_exec("node", &self.args)
            .await?
            .into())
    }
}
