use std::process::ExitCode;

use clipanion::cli;

use crate::{error::Error, project, script::ScriptEnvironment};

#[cli::command(proxy)]
#[cli::path("exec")]
pub struct Exec {
    script: String,
    args: Vec<String>,
}

impl Exec {
    #[tokio::main()]
    pub async fn execute(&self) -> Result<ExitCode, Error> {
        let mut project
            = project::Project::new(None)?;

        project
            .import_install_state()?;

        let exit_code = ScriptEnvironment::new()
            .with_project(&project)
            .run_script(&self.script, &self.args)
            .await;

        Ok(ExitCode::from(exit_code as u8))
    }
}
