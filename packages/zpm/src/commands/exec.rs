use std::process::ExitStatus;

use clipanion::cli;

use crate::{error::Error, project, script::ScriptEnvironment};

#[cli::command(proxy)]
#[cli::path("exec")]
#[cli::category("Scripting commands")]
#[cli::description("Run a shell command in the package environment")]
pub struct Exec {
    script: String,
    args: Vec<String>,
}

impl Exec {
    #[tokio::main()]
    pub async fn execute(&self) -> Result<ExitStatus, Error> {
        let mut project
            = project::Project::new(None).await?;

        project
            .lazy_install().await?;

        Ok(ScriptEnvironment::new()?
            .with_project(&project)
            .enable_shell_forwarding()
            .run_script(&self.script, &self.args)
            .await
            .into())
    }
}
