use clipanion::cli;
use zpm_config::Source;
use zpm_primitives::Ident;

use crate::{
    error::Error,
    project::{InstallMode, Project, RunInstallOptions},
};

/// Install a focused set of workspaces
///
/// This command runs an install as if the specified workspaces, and the workspaces they depend on, were the only workspaces in the project. If no
/// workspace is listed, Yarn focuses the active workspace.
///
/// This command has limited impact when using zero-installs, since the cache already contains all packages. In that case, the main difference between
/// a full install and a focused install is a few extra lines in the `.pnp.cjs` file, at the cost of additional workflow complexity.
///
/// If the `-A,--all` flag is set, the entire project will be installed. Combine with `--production` to replicate the old `yarn install --production`.
///
/// `--immutable` and `--mode=skip-build` behave as they do for `yarn install`.
///
#[cli::command]
#[cli::path("workspaces", "focus")]
#[cli::category("Workspace commands")]
pub struct WorkspacesFocus {
    /// Install all workspaces in the project
    #[cli::option("-A,--all", default = false)]
    all: bool,

    /// Exclude development dependencies from the focused install
    #[cli::option("--production", default = false)]
    production: bool,

    /// Abort if the lockfile would be modified
    #[cli::option("--immutable,--frozen-lockfile")]
    immutable: Option<bool>,

    /// Change which artifacts are generated (`skip-build` doesn't run build scripts)
    #[cli::option("--mode")]
    mode: Option<InstallMode>,

    /// Format the output as an NDJSON stream
    #[cli::option("--json", default = false)]
    json: bool,

    /// Workspaces to include in the focused install
    workspaces: Vec<Ident>,
}

impl WorkspacesFocus {
    pub async fn execute(&self) -> Result<(), Error> {
        let mut project
            = Project::new(None).await?;

        let roots = if self.all {
            project.workspaces.iter()
                .map(|workspace| workspace.name.clone())
                .collect::<Vec<_>>()
        } else if self.workspaces.is_empty() {
            vec![project.active_workspace()?.name.clone()]
        } else {
            project.workspaces.iter()
                .filter(|workspace| self.workspaces.contains(&workspace.name))
                .map(|workspace| workspace.name.clone())
                .collect::<Vec<_>>()
        };

        // A focused install never writes a partial lockfile, so only the
        // modes that link something make sense here
        if self.mode == Some(InstallMode::UpdateLockfile) {
            return Err(Error::FocusWithUpdateLockfile);
        }

        if let Some(immutable) = self.immutable {
            project.config.settings.enable_immutable_installs.force(immutable, Source::Cli);
        }

        let focused_workspaces
            = project.workspace_dependency_closure(roots, !self.production)?;

        project.run_install(RunInstallOptions {
            prune_dev_dependencies: self.production,
            roots: Some(focused_workspaces),
            mode: self.mode,
            json: self.json,
            ..Default::default()
        }).await?;

        Ok(())
    }
}
