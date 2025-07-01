use std::{os::unix::process::ExitStatusExt, process::ExitStatus};

use zpm_utils::Path;
use clipanion::cli;

use crate::{error::Error, project, script::ScriptEnvironment};

#[cli::command(default, proxy)]
#[cli::path("run")]
#[cli::category("Scripting commands")]
#[cli::description("Run a dependency binary or local script")]
pub struct Run {
    #[cli::option("-T,--top-level", default = false)]
    top_level: bool,

    #[cli::option("--error-if-missing", default = true)]
    error_if_missing: bool,

    #[cli::option("--run-cwd")]
    run_cwd: Option<Path>,

    name: String,
    args: Vec<String>,
}

impl Run {
    #[tokio::main()]
    pub async fn execute(&self) -> Result<ExitStatus, Error> {
        let mut project
            = project::Project::new(None).await?;

        project
            .lazy_install().await?;

        if self.top_level {
            project.package_cwd = Path::new();
        }

        let maybe_script
            = project.find_script(&self.name);

        if let Ok((locator, script)) = maybe_script {
            Ok(ScriptEnvironment::new()?
                .with_project(&project)
                .with_package(&project, &locator)?
                .enable_shell_forwarding()
                .run_script(&script, &self.args)
                .await
                .into())
        } else if matches!(maybe_script, Err(Error::ScriptNotFound(_)) | Err(Error::GlobalScriptNotFound(_))) {
            let maybe_binary
                = project.find_binary(&self.name);

            if let Ok(binary) = maybe_binary {
                Ok(ScriptEnvironment::new()?
                    .with_project(&project)
                    .with_package(&project, &project.active_package()?)?
                    .enable_shell_forwarding()
                    .run_binary(&binary, &self.args)
                    .await
                    .into())
            } else if let Err(Error::BinaryNotFound(binary)) = maybe_binary {
                if self.error_if_missing {
                    return Err(Error::ScriptNotFound(self.name.clone()));
                } else {
                    Ok(ExitStatus::from_raw(0))
                }
            } else {
                Err(maybe_binary.unwrap_err())
            }
        } else {
            Err(maybe_script.unwrap_err())
        }
    }
}
