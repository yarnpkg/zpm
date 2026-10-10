use std::sync::Arc;

use clipanion::cli;
use zpm_utils::Path;

use crate::{
    error::Error,
    project::{InstallMode, Project, RunInstallOptions},
};

/// Import the versions locked by pnpm
///
/// This command resolves the project dependencies again from scratch, preferring the versions locked by a `pnpm-lock.yaml` file. It's meant to be
/// used when migrating a project from pnpm, so that the migration doesn't silently upgrade the dependency tree.
///
/// Each dependency resolves to the version pnpm locked if it satisfies its range (as computed by Yarn, ie. after applying `catalogs` and
/// `resolutions`); dependencies that pnpm didn't lock, or whose range doesn't match any version pnpm locked, are resolved as usual. Versions
/// locked by pnpm are exempted from `npmMinimalAgeGate`.
///
/// Running `yarn install` in a project that has a `pnpm-lock.yaml` but no `yarn.lock` does the same thing automatically; this command is useful
/// when a `yarn.lock` was already generated, or when the pnpm lockfile is stored somewhere else. Unlike the automatic import, it discards the
/// current `yarn.lock`.
///
#[cli::command]
#[cli::path("import", "pnpm")]
#[cli::category("Dependency management")]
pub struct ImportPnpm {
    /// Path to the pnpm lockfile; defaults to `pnpm-lock.yaml` at the root of the project
    #[cli::option("--lockfile")]
    lockfile: Option<Path>,

    /// Select which install artifacts Yarn should generate
    #[cli::option("--mode")]
    mode: Option<InstallMode>,
}

impl ImportPnpm {
    pub async fn execute(&self) -> Result<(), Error> {
        let mut project
            = Project::new(None).await?;

        let pnpm_lockfile_path = match &self.lockfile {
            Some(path) => project.project_cwd.with_join(&project.shell_cwd).with_join(path),
            None => project.pnpm_lockfile_path(),
        };

        let preferred_versions
            = project.preferred_versions_from_pnpm(&pnpm_lockfile_path)?;

        project.run_install(RunInstallOptions {
            preferred_versions: Some(Arc::new(preferred_versions)),
            discard_lockfile: true,
            mode: self.mode,
            ..Default::default()
        }).await?;

        Ok(())
    }
}
