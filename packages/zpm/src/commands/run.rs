use std::{os::unix::process::ExitStatusExt, process::ExitStatus};

use clipanion::cli;

use crate::{error::Error, project, script::ScriptEnvironment};

#[cli::command(proxy)]
#[cli::path("run")]
pub struct Run {
    #[cli::option("-T,--top-level")]
    top_level: bool,

    #[cli::option("--error-if-missing", default = true)]
    error_if_missing: bool,

    name: String,
    args: Vec<String>,
}

impl Run {
    #[tokio::main()]
    pub async fn execute(&self) -> Result<ExitStatus, Error> {
        let mut project
            = project::Project::new(None)?;

        project
            .import_install_state()?;

        if self.top_level {
            project.package_cwd = ".".into();
        }

        let maybe_binary
            = project.find_binary(&self.name);

        if let Ok(binary) = maybe_binary {
            Ok(ScriptEnvironment::new()
                .with_project(&project)
                .with_package(&project, &project.active_package()?)?
                .enable_shell_forwarding()
                .run_binary(&binary, &self.args)
                .await
                .into())
        } else if let Err(Error::BinaryNotFound(_)) = maybe_binary {
            let maybe_script = project.find_script(&self.name);

            if let Ok((locator, script)) = maybe_script {
                Ok(ScriptEnvironment::new()
                    .with_project(&project)
                    .with_package(&project, &locator)?
                    .enable_shell_forwarding()
                    .run_script(&script, &self.args)
                    .await
                    .into())
            } else if let Err(Error::ScriptNotFound(script)) = maybe_script {
                if self.error_if_missing {
                    return Err(Error::ScriptNotFound(script));
                } else {
                    Ok(ExitStatus::from_raw(0))
                }
            } else {
                Err(maybe_script.unwrap_err())
            }
        } else {
            Err(maybe_binary.unwrap_err())
        }
    }
}
