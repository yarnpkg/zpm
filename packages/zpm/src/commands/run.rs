use std::{os::unix::process::ExitStatusExt, process::ExitStatus};

use zpm_utils::Path;
use clipanion::cli;

use crate::{error::Error, project, script::ScriptEnvironment};

/// Run a dependency binary or local script
///
/// This command will run a tool. The exact tool that will be executed will depend on the current state of your workspace:
///
/// - If the `scripts` field from your local package.json contains a matching script name, its definition will get executed.
///
/// - Otherwise, if one of the local workspace's dependencies exposes a binary with a matching name, this binary will get executed.
///
/// - Otherwise, if the specified name contains a colon character and if one of the workspaces in the project contains exactly one script with a
///   matching name, then this script will get executed.
///
/// Whatever happens, the cwd of the spawned process will be the workspace that declares the script (which makes it possible to call commands
/// cross-workspaces using the third syntax).
///
#[cli::command(default, proxy)]
#[cli::path("run")]
#[cli::category("Scripting commands")]
pub struct Run {
    /// If set, the script or binary used will be the one in the top-level workspace
    #[cli::option("-T,--top-level", default = false)]
    top_level: bool,

    // If set, only binaries will be considered
    #[cli::option("-B,--binaries-only", default = false)]
    binaries_only: bool,

    /// If set (the default), an error will be returned if the script or binary is not found
    #[cli::option("--error-if-missing", default = true)]
    error_if_missing: bool,

    /// The directory in which to run the script or binary
    #[cli::option("--run-cwd")]
    run_cwd: Option<Path>,

    /// Forwarded to the underlying Node process when executing a binary
    #[cli::option("--inspect")]
    inspect: Option<Option<String>>,

    /// Forwarded to the underlying Node process when executing a binary
    #[cli::option("--inspect-brk")]
    inspect_brk: Option<Option<String>>,

    /// Forwarded to the underlying Node process when executing a binary
    #[cli::option("--inspect-wait")]
    inspect_wait: Option<Option<String>>,

    /// Forwarded to the underlying Node process when executing a binary
    #[cli::option("--require")]
    require: Option<String>,

    /// Name of the script or binary to run
    name: String,

    /// Arguments to pass to the script or binary
    args: Vec<String>,
}

impl Run {
    pub async fn execute(&self) -> Result<ExitStatus, Error> {
        let mut project
            = project::Project::new(None).await?;

        project
            .lazy_install().await?;

        if self.top_level {
            project.package_cwd = Path::new();
        }

        let get_node_args = || {
            let mut node_args = Vec::new();

            if let Some(inspect) = &self.inspect {
                node_args.push(match inspect {
                    Some(address) => format!("--inspect={}", address),
                    None => "--inspect".to_owned(),
                });
            }

            if let Some(inspect_brk) = &self.inspect_brk {
                node_args.push(match inspect_brk {
                    Some(address) => format!("--inspect-brk={}", address),
                    None => "--inspect-brk".to_owned(),
                });
            }

            if let Some(inspect_wait) = &self.inspect_wait {
                node_args.push(match inspect_wait {
                    Some(address) => format!("--inspect-wait={}", address),
                    None => "--inspect-wait".to_owned(),
                });
            }

            if let Some(require) = &self.require {
                node_args.push(format!("--require={}", require));
            }

            node_args
        };

        let execute_binary = async |error_script_not_found: bool| {
            let maybe_binary
                = project.find_binary(&self.name);

            if let Ok(binary) = maybe_binary {
                Ok(ScriptEnvironment::new()?
                    .with_project(&project)
                    .with_package(&project, &project.active_package()?)?
                    .with_node_args(get_node_args())
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
                let node_args = get_node_args();

                // TODO: Investigate whether --require should be forwarded to scripts via NODE_OPTIONS.
                if !node_args.is_empty() {
                    return Err(Error::InvalidRunScriptOptions(node_args));
                }

                Ok(ScriptEnvironment::new()?
                    .with_project(&project)
                    .with_package(&project, &locator)?
                    .enable_shell_forwarding()
                    .run_script(&script, &self.args)
                    .await?
                    .into())
            },

            Err(Error::ScriptNotFound(_)) | Err(Error::GlobalScriptNotFound(_))
                => execute_binary(true).await,

            Err(err) => Err(err),
        }
    }
}
