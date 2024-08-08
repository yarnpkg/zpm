use std::process::ExitCode;

use clipanion::cli;

use crate::{error::{self, Error}, project, script::ScriptEnvironment};

#[cli::command(proxy)]
#[cli::path("run")]
pub struct Run {
    name: String,
    args: Vec<String>,
}

impl Run {
    #[tokio::main()]
    pub async fn execute(&self) -> error::Result<ExitCode> {
        let mut project
            = project::Project::new(None)?;

        project
            .import_install_state()?;

        let maybe_binary
            = project.find_binary(&self.name);

        if let Ok(binary_path) = maybe_binary {
            let exit_code = ScriptEnvironment::new()
                .with_project(&project)
                .with_package(&project, &project.active_package()?)?
                .run_exec(&binary_path.to_string(), &self.args)
                .await;

            Ok(ExitCode::from(exit_code as u8))
        } else if let Err(Error::BinaryNotFound(_)) = maybe_binary {
            let (locator, script)
                = project.find_script(&self.name)?;

            let exit_code = ScriptEnvironment::new()
                .with_project(&project)
                .with_package(&project, &locator)?
                .run_script(&script)
                .await;

            Ok(ExitCode::from(exit_code as u8))
        } else {
            Err(maybe_binary.unwrap_err())
        }
    }
}
