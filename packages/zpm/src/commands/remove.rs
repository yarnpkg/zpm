use std::collections::HashSet;

use clipanion::cli;
use wax::{Glob, Program};

use crate::{config::Config, error::Error, primitives::Ident, project::{Project, RunInstallOptions, Workspace}};

#[cli::command]
#[cli::path("remove")]
pub struct Remove {
    #[cli::option("-A,--all", default = false)]
    all: bool,

    identifiers: Vec<Ident>,
}

impl Remove {
    #[tokio::main()]
    pub async fn execute(&self) -> Result<(), Error> {
        let mut project
            = Project::new(None).await?;

        let ident_globs = self.identifiers.iter()
            .map(|ident| Glob::new(ident.as_str()).unwrap())
            .collect::<Vec<_>>();

        if self.all {
            for workspace in project.workspaces.iter_mut() {
                self.remove_dependencies_from_manifest(&project.config, workspace, &ident_globs)?;
            }
        } else {
            let active_workspace_idx
                = project.active_workspace_idx()?;

            let active_workspace
                = &mut project.workspaces[active_workspace_idx];

            self.remove_dependencies_from_manifest(&project.config, active_workspace, &ident_globs)?;
        }

        project.run_install(RunInstallOptions {
            check_resolutions: false,
            refresh_lockfile: false,
        }).await?;

        Ok(())
    }

    fn remove_dependencies_from_manifest(&self, config: &Config, workspace: &mut Workspace, ident_globs: &[Glob]) -> Result<(), Error> {
        let all_dependencies = workspace.manifest.remote.dependencies.keys()
            .chain(workspace.manifest.remote.optional_dependencies.keys())
            .chain(workspace.manifest.remote.peer_dependencies.keys())
            .chain(workspace.manifest.dev_dependencies.keys())
            .collect::<HashSet<_>>();

        let mut removed_dependencies = all_dependencies.into_iter()
            .filter(|ident| ident_globs.iter().any(|glob| glob.is_match(ident.as_str())))
            .cloned()
            .collect::<Vec<_>>();

        if config.project.enable_auto_types.value {
            removed_dependencies = removed_dependencies.into_iter()
                .flat_map(|ident| vec![ident.type_ident(), ident])
                .collect::<Vec<_>>();
        }

        for ident in removed_dependencies.iter() {
            workspace.manifest.remote.dependencies.remove(ident);
            workspace.manifest.remote.optional_dependencies.remove(ident);
            workspace.manifest.remote.peer_dependencies.remove(ident);
            workspace.manifest.dev_dependencies.remove(ident);
        }

        workspace.write_manifest()?;

        Ok(())
    }
}
