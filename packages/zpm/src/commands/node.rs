use std::process::ExitCode;

use clipanion::cli;

use crate::{error::Error, project, script::ScriptEnvironment};

#[cli::command(proxy)]
#[cli::path("node")]
pub struct Node {
    args: Vec<String>,
}

impl Node {
    #[tokio::main()]
    pub async fn execute(&self) -> Result<ExitCode, Error> {
        let project
            = project::Project::new(None)?;

        let exit_code = ScriptEnvironment::new()
            .with_project(&project)
            .run_exec("node", &self.args)
            .await;

        Ok(ExitCode::from(exit_code as u8))
    }
}
