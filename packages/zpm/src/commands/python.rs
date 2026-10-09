use std::process::ExitStatus;

use clipanion::cli;
use zpm_config::IslandLinker;
use zpm_utils::ToFileString;

use crate::{error::Error, project, script::ScriptEnvironment};

fn prepend_env_path(key: &str, value: &str, separator: char) -> String {
    let current
        = std::env::var(key).ok()
            .filter(|v| !v.is_empty());

    match current {
        Some(current) => format!("{}{}{}", value, separator, current),
        None => value.to_string(),
    }
}

fn active_workspace_venv(project: &project::Project) -> Option<zpm_utils::Path> {
    let workspace
        = project.active_workspace().ok()?;

    let in_venv_island = project.config.settings.unstable_islands
        .values()
        .any(|island| {
            island.linker.value == IslandLinker::Venv
                && island.workspaces.iter().any(|glob| glob.value.check(&workspace.name))
        });

    if !in_venv_island {
        return None;
    }

    Some(
        project.project_cwd
            .with_join(&workspace.rel_path)
            .with_join_str(".venv"),
    )
}

/// Run a Python process within the project's environment
///
/// This command mirrors `yarn node`, but for Python. When called from a workspace that belongs to an island using the `venv` linker, it runs the
/// workspace's `.venv/bin/python` (installing the project first if needed), with `VIRTUAL_ENV` set and `.venv/bin` prepended to the `PATH`.
///
#[cli::command(proxy)]
#[cli::path("python")]
#[cli::category("Scripting commands")]
pub struct Python {
    /// Arguments to pass to Python
    args: Vec<String>,
}

impl Python {
    pub async fn execute(&self) -> Result<ExitStatus, Error> {
        let mut project
            = project::Project::new(None).await?;

        project
            .lazy_install().await?;

        let mut env = ScriptEnvironment::new()?
            .with_project(&project)
            .with_package(&project, &project.active_package()?)?
            .enable_shell_forwarding()
            .enable_signal_delegation();

        let mut program
            = "python".to_string();

        if let Some(venv_path) = active_workspace_venv(&project) {
            let bin_path
                = venv_path.with_join_str("bin");

            let path
                = prepend_env_path("PATH", &bin_path.to_file_string(), ':');

            env = env
                .with_env_variable("VIRTUAL_ENV", &venv_path.to_file_string())
                .with_env_variable("PATH", &path);

            program = bin_path.with_join_str("python").to_file_string();
        }

        let result
            = env.run_exec(&program, &self.args).await?;

        Ok(result.into())
    }
}
