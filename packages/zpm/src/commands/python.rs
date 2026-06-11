use std::{collections::BTreeSet, process::ExitStatus};

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

fn discover_site_packages_paths(venv_path: &zpm_utils::Path) -> Vec<zpm_utils::Path> {
    let mut paths
        = BTreeSet::new();

    let lib_path
        = venv_path.with_join_str("lib");

    if let Ok(read_dir) = lib_path.fs_read_dir() {
        for entry in read_dir.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };

            if !file_type.is_dir() {
                continue;
            }

            let dirname
                = entry.file_name();

            let dirname
                = dirname.to_string_lossy();

            if !dirname.starts_with("python") {
                continue;
            }

            let site_packages_path
                = lib_path
                    .with_join_str(dirname.as_ref())
                    .with_join_str("site-packages");

            if site_packages_path.fs_exists() {
                paths.insert(site_packages_path);
            }
        }
    }

    let legacy_site_packages_path
        = lib_path.with_join_str("site-packages");

    if legacy_site_packages_path.fs_exists() {
        paths.insert(legacy_site_packages_path);
    }

    paths.into_iter().collect()
}

fn build_site_packages_pythonpath(site_packages_paths: &[zpm_utils::Path], separator: char) -> String {
    let mut entries
        = Vec::new();

    for site_packages_path in site_packages_paths {
        entries.push(site_packages_path.to_file_string());

        if let Ok(read_dir) = site_packages_path.fs_read_dir() {
            for entry in read_dir.flatten() {
                let Ok(file_type) = entry.file_type() else {
                    continue;
                };

                if !file_type.is_dir() {
                    continue;
                }

                let dirname
                    = entry.file_name();

                let dirname
                    = dirname.to_string_lossy();

                if dirname.starts_with('.') {
                    continue;
                }

                entries.push(
                    site_packages_path
                        .with_join_str(dirname.as_ref())
                        .to_file_string(),
                );
            }
        }
    }

    entries.join(&separator.to_string())
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
/// This command mirrors `yarn node`, but for Python. When called from a workspace that belongs to an island using the `venv` linker, it sets up a
/// virtualenv-like environment (`VIRTUAL_ENV`, `PYTHONPATH`, and `PATH`) so Python can resolve packages from `.venv/lib/site-packages`.
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

        let mut python_program
            = "python".to_string();

        if let Some(venv_path) = active_workspace_venv(&project) {
            let site_packages_paths
                = discover_site_packages_paths(&venv_path);

            let bin_path = if cfg!(windows) {
                venv_path.with_join_str("Scripts")
            } else {
                venv_path.with_join_str("bin")
            };

            let path_separator = if cfg!(windows) {
                ';'
            } else {
                ':'
            };

            let path
                = prepend_env_path("PATH", &bin_path.to_file_string(), path_separator);

            let venv_python_path
                = bin_path.with_join_str("python");

            if venv_python_path.fs_exists() {
                python_program = venv_python_path.to_file_string();
            }

            let pythonpath
                = prepend_env_path(
                    "PYTHONPATH",
                    &build_site_packages_pythonpath(&site_packages_paths, path_separator),
                    path_separator,
                );

            env = env
                .with_env_variable("VIRTUAL_ENV", &venv_path.to_file_string())
                .with_env_variable("PYTHONPATH", &pythonpath)
                .with_env_variable("PATH", &path);
        }

        let result
            = env.run_exec(&python_program, &self.args).await?;

        Ok(result.into())
    }
}
