use std::collections::HashSet;

use clipanion::cli;
use wax::{Glob, Program};
use zpm_config::Configuration;
use zpm_parsers::{document::Document, JsonDocument};
use zpm_primitives::Ident;
use zpm_utils::ToFileString;

use crate::{
    error::Error,
    project::{InstallMode, Project, RunInstallOptions, Workspace},
};

/// Remove dependencies from the project
#[cli::command]
#[cli::path("remove")]
#[cli::category("Dependency management")]
pub struct Remove {
    #[cli::option("-A,--all", default = false)]
    all: bool,

    #[cli::option("--mode")]
    mode: Option<InstallMode>,

    // ---

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
                self.remove_dependencies_from_manifest(&project.config, workspace, &ident_globs)?;
            }
        } else {
            let active_workspace_idx
                = project.active_workspace_idx()?;

            let active_workspace
                = &project.workspaces[active_workspace_idx];

            self.remove_dependencies_from_manifest(&project.config, active_workspace, &ident_globs)?;
        }

        let mut project
            = Project::new(None).await?;

        project.run_install(RunInstallOptions {
            mode: self.mode,
            ..Default::default()
        }).await?;

        Ok(())
    }

    fn remove_dependencies_from_manifest(&self, config: &Configuration, workspace: &Workspace, ident_globs: &[Glob]) -> Result<(), Error> {
        let all_dependencies = workspace.manifest.remote.dependencies.keys()
            .chain(workspace.manifest.remote.optional_dependencies.keys())
            .chain(workspace.manifest.remote.peer_dependencies.keys())
            .chain(workspace.manifest.dev_dependencies.keys())
            .collect::<HashSet<_>>();

        let mut removed_dependencies = all_dependencies.into_iter()
            .filter(|ident| ident_globs.iter().any(|glob| glob.is_match(ident.as_str())))
            .cloned()
            .collect::<Vec<_>>();

        if config.settings.enable_auto_types.value {
            removed_dependencies = removed_dependencies.into_iter()
                .flat_map(|ident| vec![ident.type_ident(), ident])
                .collect::<Vec<_>>();
        }

        let manifest_path = workspace.path
            .with_join_str("package.json");

        let manifest_content = manifest_path
            .fs_read_prealloc()?;

        let mut document
            = JsonDocument::new(manifest_content)?;

        for ident in removed_dependencies.iter() {
            document.set_path(
                &zpm_parsers::Path::from_segments(vec!["dependencies".to_string(), ident.to_file_string()]),
                zpm_parsers::Value::Undefined,
            )?;

            document.set_path(
                &zpm_parsers::Path::from_segments(vec!["optionalDependencies".to_string(), ident.to_file_string()]),
                zpm_parsers::Value::Undefined,
            )?;

            document.set_path(
                &zpm_parsers::Path::from_segments(vec!["peerDependencies".to_string(), ident.to_file_string()]),
                zpm_parsers::Value::Undefined,
            )?;

            document.set_path(
                &zpm_parsers::Path::from_segments(vec!["devDependencies".to_string(), ident.to_file_string()]),
                zpm_parsers::Value::Undefined,
            )?;
        }

        manifest_path
            .fs_change(&document.input, false)?;

        Ok(())
    }
}
