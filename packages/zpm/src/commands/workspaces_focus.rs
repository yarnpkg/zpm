use std::collections::BTreeSet;

use clipanion::cli;
use zpm_primitives::Ident;

use crate::{
    error::Error,
    project::{Project, RunInstallOptions, Workspace},
};

#[cli::command]
#[cli::path("workspaces", "focus")]
#[cli::category("Workspace commands")]
#[cli::description("Install a single workspace and its dependencies")]
pub struct WorkspacesFocus {
    #[cli::option("-A,--all", default = false)]
    all: bool,

    #[cli::option("--production", default = false)]
    production: bool,

    #[cli::option("--json", default = false)]
    json: bool,

    workspaces: Vec<Ident>,
}

impl WorkspacesFocus {
    #[tokio::main()]
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
