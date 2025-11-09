use std::process::ExitStatus;

use clipanion::cli;

use crate::{error::Error, project, script::ScriptEnvironment};

/// Run a Node.js process within the project's environment
///
/// This command simply runs Node.js. It also makes sure to call it in a way that's compatible with the current project (for example, on Yarn PnP
/// projects the environment will be setup in such a way that Yarn PnP will be correctly injected into the environment).
///
#[cli::command(proxy)]
#[cli::path("node")]
#[cli::category("Scripting commands")]
pub struct Node {
    /// The arguments to pass to the Node.js process
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
