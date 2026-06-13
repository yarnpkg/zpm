use std::{os::unix::process::ExitStatusExt, process::ExitStatus};

use indexmap::IndexMap;
use zpm_parsers::JsonDocument;
use zpm_utils::Path;
use clipanion::cli;

use crate::{commands::tasks::run_silent_dependencies::TaskRunSilentDependencies, error::Error, project, script::ScriptEnvironment};
use super::tasks as task_run;

/// List scripts from the current workspace
#[cli::command]
#[cli::path("run")]
#[cli::category("Scripting commands")]
pub struct RunList {
    /// Format the output as an NDJSON stream
    #[cli::option("--json", default = false)]
    json: bool,
}

#[derive(serde::Deserialize)]
struct ScriptsManifest {
    #[serde(default)]
    scripts: IndexMap<String, String>,
}

impl RunList {
    pub async fn execute(&self) -> Result<ExitStatus, Error> {
        let mut project
            = project::Project::new(None).await?;

        project
            .lazy_install().await?;

        list_scripts(&project, self.json)?;
        Ok(ExitStatus::from_raw(0))
    }
}

/// Run a dependency binary or local script
///
/// This command runs a tool selected from the current workspace:
///
/// - If the local package.json `scripts` field contains a matching script name, Yarn executes that script.
///
/// - Otherwise, if a `taskfile` exists in the workspace and contains a task with the matching name, Yarn runs that task
///   (including all its dependencies in the correct order).
///
/// - Otherwise, if one of the local workspace's dependencies exposes a binary with a matching name, Yarn runs that binary.
///
/// - Otherwise, if the specified name contains a colon character and if one of the workspaces in the project contains exactly one script with a
///   matching name, Yarn runs that script.
///
/// Script commands run from the workspace that declares them. Dependency binaries run from the current working directory unless `--run-cwd` is set.
#[cli::command(default, proxy)]
#[cli::path("run")]
#[cli::category("Scripting commands")]
pub struct Run {
    /// Resolve scripts and binaries from the top-level workspace
    #[cli::option("-T,--top-level", default = false)]
    top_level: bool,

    /// Only consider dependency binaries, not package scripts or tasks
    #[cli::option("-B,--binaries-only", default = false)]
    binaries_only: bool,

    /// Return an error when the script, task, or binary cannot be found
    #[cli::option("--error-if-missing", default = true)]
    error_if_missing: bool,

    /// Directory from which to run the selected script or binary
    #[cli::option("--run-cwd")]
    run_cwd: Option<Path>,

    /// Forward `--inspect` to Node.js when executing a Node binary
    #[cli::option("--inspect")]
    inspect: Option<Option<String>>,

    /// Forward `--inspect-brk` to Node.js when executing a Node binary
    #[cli::option("--inspect-brk")]
    inspect_brk: Option<Option<String>>,

    /// Forward `--inspect-wait` to Node.js when executing a Node binary
    #[cli::option("--inspect-wait")]
    inspect_wait: Option<Option<String>>,

    /// Preload a module through Node.js `--require` when executing a Node binary
    #[cli::option("--require")]
    require: Option<String>,

    /// Name of the script or binary to run
    name: String,

    /// Arguments to pass to the script or binary
    args: Vec<String>,
}

fn list_scripts(project: &project::Project, json: bool) -> Result<(), Error> {
    let active_workspace = project.active_workspace()?;
    let manifest_path = active_workspace.manifest_path();

    let manifest_text = manifest_path
        .fs_read_text()
        .unwrap_or_default();

    let manifest: ScriptsManifest = JsonDocument::hydrate_from_str(&manifest_text)
        .unwrap_or_else(|_| ScriptsManifest { scripts: IndexMap::new() });

    if json {
        for (name, script) in &manifest.scripts {
            let entry = serde_json::json!({"name": name, "script": script});
            println!("{}", serde_json::to_string(&entry).unwrap());
        }

        return Ok(());
    }

    let max_name_len = manifest.scripts.keys()
        .map(|k| k.len())
        .max()
        .unwrap_or(0);

    for (name, script) in &manifest.scripts {
        let padding = " ".repeat(max_name_len.saturating_sub(name.len()));
        println!("{}{}   '{}'", name, padding, script);
    }

    Ok(())
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

        let name = self.name.as_str();

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
                = project.find_binary(name);

            if let Ok(binary) = maybe_binary {
                Ok(ScriptEnvironment::new()?
                    .with_project(&project)
                    .with_package(&project, &project.active_package()?)?
                    .with_node_args(get_node_args())
                    .enable_shell_forwarding()
                    .enable_signal_delegation()
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

        match project.find_script(name) {
            Ok((locator, script)) => {
                let node_args = get_node_args();

                // TODO: Investigate whether --require should be forwarded to scripts via NODE_OPTIONS.
                if !node_args.is_empty() {
                    return Err(Error::InvalidRunScriptOptions(node_args));
                }

                Ok(ScriptEnvironment::new()?
                    .with_project(&project)
                    .with_package(&project, &locator)?
                    .with_env_variable("npm_lifecycle_event", name)
                    .enable_shell_forwarding()
                    .enable_signal_delegation()
                    .run_script(&script, &self.args)
                    .await?
                    .into())
            },

            Err(Error::ScriptNotFound(_)) | Err(Error::GlobalScriptNotFound(_)) => {
                if task_run::task_exists(&project, name) {
                    let task_run_silent_dependencies
                        = TaskRunSilentDependencies::new(&self.cli_environment, name.to_string(), self.args.clone());

                    return task_run_silent_dependencies.execute().await;
                }

                execute_binary(true).await
            }

            Err(err) => Err(err),
        }
    }
}
