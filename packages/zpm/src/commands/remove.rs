use std::collections::BTreeSet;

use clipanion::cli;
use wax::{Glob, Program};
use zpm_primitives::Ident;

use crate::{
    error::Error,
    manifest::dependencies::{ManifestScope, ManifestSet},
    project::{InstallMode, Project, RunInstallOptions, Workspace},
};

/// Remove dependencies from the project
///
/// This command removes the packages matching the specified names or glob patterns from the current workspace, then runs an install.
///
/// If the `--mode=<mode>` option is set, Yarn will change which artifacts are generated. The modes currently supported are:
///
/// - `skip-build` will not run the build scripts at all. Note that this is different from setting `enableScripts` to false because the latter will
///   disable build scripts, and thus affect the content of the artifacts generated on disk, whereas the former will just disable the build step but
///   not the scripts themselves, which just won't run.
///
/// - `update-lockfile` will skip the link step altogether, and only fetch packages that are missing from the lockfile (or that have no associated
///   checksums). This mode is typically used by tools like Renovate or Dependabot to keep a lockfile up-to-date without incurring the full install
///   cost.
///
/// This command accepts glob patterns as arguments. Make sure to escape the patterns, to prevent your own shell from trying to expand them.
///
/// ## Examples
///
/// Remove a dependency from the current project
///
/// `yarn remove lodash`
///
/// Remove a dependency from all workspaces at once
///
/// `yarn remove lodash --all`
///
/// Remove all dependencies starting with `eslint-`
///
/// `yarn remove 'eslint-*'`
///
/// Remove all dependencies with the `@babel` scope
///
/// `yarn remove '@babel/*'`
///
#[cli::command]
#[cli::path("remove")]
#[cli::category("Dependency management")]
pub struct Remove {
    /// Remove matching dependencies from all workspaces in the project
    #[cli::option("-A,--all", default = false)]
    all: bool,

    /// Select which install artifacts Yarn should generate
    #[cli::option("--mode")]
    mode: Option<InstallMode>,

    // ---

    /// Package names or glob patterns to remove
    identifiers: Vec<Ident>,
}

impl Remove {
    pub async fn execute(&self) -> Result<(), Error> {
        let project
            = Project::new(None).await?;

        let ident_globs = self.identifiers.iter()
            .map(|ident| Glob::new(ident.as_str()).unwrap())
            .collect::<Vec<_>>();

        if self.all {
            for workspace in &project.workspaces {
                self.remove_dependencies(&project, workspace, ManifestScope::All, &ident_globs)?;
            }
        } else {
            let active_workspace_idx
                = project.active_workspace_idx()?;

            let active_workspace
                = &project.workspaces[active_workspace_idx];

            self.remove_dependencies(&project, active_workspace, ManifestScope::Active, &ident_globs)?;
        }

        let mut project
            = Project::new(None).await?;

        project.run_install(RunInstallOptions {
            mode: self.mode,
            ..Default::default()
        }).await?;

        Ok(())
    }

    /// Removes the matching dependencies from the manifests of a workspace:
    /// its package.json, and the ones of the other ecosystems it's part of.
    fn remove_dependencies(&self, project: &Project, workspace: &Workspace, scope: ManifestScope, ident_globs: &[Glob]) -> Result<(), Error> {
        let mut manifests
            = ManifestSet::open(project, workspace, scope)?;

        for manifest in manifests.iter_mut() {
            let mut removed_dependencies = manifest.dependencies().into_iter()
                .map(|dependency| dependency.descriptor.ident)
                .filter(|ident| ident_globs.iter().any(|glob| glob.is_match(ident.as_str())))
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();

            if project.config.settings.enable_auto_types.value {
                removed_dependencies = removed_dependencies.into_iter()
                    .flat_map(|ident| vec![ident.type_ident(), ident])
                    .collect::<Vec<_>>();
            }

            for ident in &removed_dependencies {
                manifest.remove(ident)?;
            }
        }

        manifests.save()
    }
}
