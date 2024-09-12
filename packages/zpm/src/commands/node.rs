use std::process::ExitStatus;

use clipanion::cli;

use crate::{error::Error, project, script::ScriptEnvironment};

#[cli::command(proxy)]
#[cli::path("node")]
pub struct Node {
    args: Vec<String>,
}

impl Node {
    #[tokio::main()]
    pub async fn execute(&self) -> Result<ExitStatus, Error> {
        let project
            = project::Project::new(None)?;

        Ok(ScriptEnvironment::new()
            .with_project(&project)
            .run_exec("node", &self.args)
            .await
            .into())
    }
}
