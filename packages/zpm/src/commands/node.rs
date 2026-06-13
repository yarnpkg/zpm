use std::process::ExitStatus;

use clipanion::cli;

use crate::{error::Error, project, script::ScriptEnvironment};

/// Run a Node.js process within the project's environment
///
/// This command runs Node.js with the current project's environment, including Plug'n'Play injection when the project uses PnP.
///
#[cli::command(proxy)]
#[cli::path("node")]
#[cli::category("Scripting commands")]
pub struct Node {
    /// Arguments to pass to Node.js
    args: Vec<String>,
}

impl Node {
    pub async fn execute(&self) -> Result<ExitStatus, Error> {
        let mut project
            = project::Project::new(None).await?;

        project
            .lazy_install().await?;

        Ok(ScriptEnvironment::new()?
            .with_project(&project)
            .with_package(&project, &project.active_package()?)?
            .enable_shell_forwarding()
            .enable_signal_delegation()
            .run_exec("node", &self.args)
            .await?
            .into())
    }
}
