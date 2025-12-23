use std::process::ExitStatus;

use clipanion::cli;

use crate::{error::Error, project, script::ScriptEnvironment};

/// Run a shell command in the package environment
///
/// This command simply executes a shell command within the context of the root directory of the active workspace.
///
/// It also makes sure to call it in a way that's compatible with the current project (for example, on PnP projects the environment will be setup in
/// such a way that PnP will be correctly injected into the environment).
///
#[cli::command(proxy)]
#[cli::path("exec")]
#[cli::category("Scripting commands")]
pub struct Exec {
    /// The shell command to execute
    script: String,

    /// The arguments to pass to the script
    args: Vec<String>,
}

impl Exec {
    pub async fn execute(&self) -> Result<ExitStatus, Error> {
        let mut project
            = project::Project::new(None).await?;

        project
            .lazy_install().await?;

        Ok(ScriptEnvironment::new()?
            .with_project(&project)
            .with_package(&project, &project.active_package()?)?
            .enable_shell_forwarding()
            .run_script(&self.script, &self.args)
            .await?
            .into())
    }
}
