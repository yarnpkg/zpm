use std::process::ExitStatus;

use clipanion::cli;
use zpm_utils::Path;

use crate::{error::Error, project, script::ScriptEnvironment};

/// Run a shell command in the package environment
///
/// This command executes a shell command from the current directory, with the environment prepared for the active workspace.
///
/// The spawned process receives the same project environment as scripts and binaries, including Plug'n'Play injection when the project uses PnP.
///
#[cli::command(proxy)]
#[cli::path("exec")]
#[cli::category("Scripting commands")]
pub struct Exec {
    /// Shell command to execute
    script: String,

    /// Arguments to pass to the command
    args: Vec<String>,
}

impl Exec {
    pub async fn execute(&self) -> Result<ExitStatus, Error> {
        let mut project
            = project::Project::new(None).await?;

        project
            .lazy_install().await?;

        let env = ScriptEnvironment::new()?
            .with_project(&project)
            .with_package(&project, &project.active_package()?)?
            .with_cwd(Path::current_dir()?)
            .enable_shell_forwarding()
            .enable_signal_delegation();
        let (mut env, _)
            = super::python::activate_workspace_venv(&project, env);

        Ok(env
            .run_script(&self.script, &self.args)
            .await?
            .into())
    }
}
