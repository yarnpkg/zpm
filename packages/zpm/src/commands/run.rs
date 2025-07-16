use std::{os::unix::process::ExitStatusExt, process::ExitStatus};

use zpm_utils::Path;
use clipanion::cli;

use crate::{error::Error, project, script::ScriptEnvironment};

#[cli::command(proxy)]
#[cli::path("run")]
#[cli::category("Scripting commands")]
#[cli::description("Run a dependency binary or local script")]
pub struct Run {
    #[cli::option("-T,--top-level", default = false)]
    top_level: bool,

    #[cli::option("-B,--binaries-only", default = false)]
    binaries_only: bool,

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

        let execute_binary = async |error_script_not_found: bool| {
            let maybe_binary
                = project.find_binary(&self.name);

            if let Ok(binary) = maybe_binary {
                Ok(ScriptEnvironment::new()?
                    .with_project(&project)
                    .with_package(&project, &project.active_package()?)?
                    .enable_shell_forwarding()
                    .run_binary(&binary, &self.args)
                    .await?
                    .into())
            } else if let Err(Error::BinaryNotFound(name)) = maybe_binary {
                if self.error_if_missing {
                    Err(if error_script_not_found {
                        Error::ScriptNotFound(name)
                    } else {
                        Error::BinaryNotFound(name)
                    })
                } else {
                    Ok(ExitStatus::from_raw(0))
                }
            } else {
                Err(maybe_binary.unwrap_err())
            }
        };

        if self.binaries_only {
            return execute_binary(false).await;
        }

        match project.find_script(&self.name) {
            Ok((locator, script)) => {
                Ok(ScriptEnvironment::new()?
                    .with_project(&project)
                    .with_package(&project, &locator)?
                    .enable_shell_forwarding()
                    .run_script(&script, &self.args)
                    .await?
                    .into())
            },

            Err(Error::ScriptNotFound(_)) | Err(Error::GlobalScriptNotFound(_)) => execute_binary(true).await,

            Err(err) => Err(err),
        }
    }
}
