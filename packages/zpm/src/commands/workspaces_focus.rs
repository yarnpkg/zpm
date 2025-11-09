use std::collections::BTreeSet;

use clipanion::cli;
use zpm_primitives::Ident;

use crate::{
    error::Error,
    project::{Project, RunInstallOptions, Workspace},
};

/// Install a single workspace and its dependencies
///
/// This command will run an install as if the specified workspaces (and all other workspaces they depend on) were the only ones in the project. If
/// no workspaces are explicitly listed, the active one will be assumed.
///
/// Note that this command is only very moderately useful when using zero-installs, since the cache will contain all the packages anyway - meaning
/// that the only difference between a full install and a focused install would just be a few extra lines in the .pnp.cjs file, at the cost of
/// introducing an extra complexity.
///
/// If the `-A,--all` flag is set, the entire project will be installed. Combine with `--production` to replicate the old `yarn install --production`.
///
#[cli::command]
#[cli::path("workspaces", "focus")]
#[cli::category("Workspace commands")]
pub struct WorkspacesFocus {
    /// Install all workspaces in the project
    #[cli::option("-A,--all", default = false)]
    all: bool,

    /// Only install production dependencies
    #[cli::option("--production", default = false)]
    production: bool,

    /// Format the output as an NDJSON stream
    #[cli::option("--json", default = false)]
    json: bool,

    /// The workspaces to focus on
    workspaces: Vec<Ident>,
}

impl WorkspacesFocus {
    pub async fn execute(&self) -> Result<(), Error> {
        let mut project
            = Project::new(None).await?;

        let workspaces = if self.all {
            project.workspaces.iter().collect::<Vec<_>>()
        } else if self.workspaces.is_empty() {
            vec![project.active_workspace()?]
        } else {
            project.workspaces.iter().filter(|w| self.workspaces.contains(&w.name)).collect::<Vec<_>>()
        };

        let mut process_queue: Vec<&Workspace>
            = workspaces.clone();
        let mut processed_queue
            = BTreeSet::from_iter(process_queue.iter().map(|w| &w.name));

        while let Some(workspace) = process_queue.pop() {
            let mut relevant_dependencies
                = workspace.manifest.remote.dependencies.iter()
                    .map(|(_, d)| d)
                    .collect::<Vec<_>>();

            if !self.production {
                relevant_dependencies.extend(workspace.manifest.dev_dependencies.iter()
                    .map(|(_, d)| d));
            }

            for dependency in relevant_dependencies {
                if let Some(workspace) = project.try_workspace_by_descriptor(&dependency)? {
                    if processed_queue.insert(&workspace.name) {
                        process_queue.push(workspace);
                    }
                }
            }
        }

        project.run_install(RunInstallOptions {
            prune_dev_dependencies: self.production,
            roots: Some(processed_queue.into_iter().cloned().collect()),
            ..Default::default()
        }).await?;

        Ok(())
    }
}
