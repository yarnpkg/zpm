use std::collections::BTreeSet;

use clipanion::cli;
use zpm_parsers::{JsonFormatter, Value};
use zpm_primitives::Ident;
use zpm_semver::RangeKind;
use zpm_utils::ToFileString;

use crate::{
    descriptor_loose::{self, LooseDescriptor},
    error::Error,
    install::InstallContext,
    project::{InstallMode, Project, RunInstallOptions, Workspace}
};

#[cli::command]
#[cli::path("up")]
#[cli::category("Dependency management")]
#[cli::description("Update dependencies to the latest versions")]
pub struct Up {
    #[cli::option("-F,--fixed", default = false)]
    fixed: bool,

    #[cli::option("-E,--exact", default = false)]
    exact: bool,

    #[cli::option("-T,--tilde", default = false)]
    tilde: bool,

    #[cli::option("-C,--caret", default = false)]
    caret: bool,

    // ---

    #[cli::option("--mode")]
    mode: Option<InstallMode>,

    // ---

    descriptors: Vec<LooseDescriptor>,
}

impl Up {
    #[tokio::main()]
    pub async fn execute(&self) -> Result<(), Error> {
        let project
            = Project::new(None).await?;

        let all_idents = project.workspaces.iter()
            .flat_map(|workspace| self.list_workspace_idents(workspace))
            .collect::<BTreeSet<_>>();

        let expanded_descriptors = self.descriptors.iter()
            .flat_map(|descriptor| descriptor.expand(&all_idents))
            .collect::<Vec<_>>();

        let range_kind = if self.fixed {
            RangeKind::Exact
        } else if self.exact {
            RangeKind::Exact
        } else if self.tilde {
            RangeKind::Tilde
        } else if self.caret {
            RangeKind::Caret
        } else {
            project.config.settings.default_semver_range_prefix.value
        };

        let resolve_options = descriptor_loose::ResolveOptions {
            active_workspace_ident: project.active_workspace()?.name.clone(),
            range_kind,
            resolve_tags: !self.fixed,
        };

        let package_cache
            = project.package_cache()?;

        let install_context = InstallContext::default()
            .with_package_cache(Some(&package_cache))
            .with_project(Some(&project));

        let descriptors
            = LooseDescriptor::resolve_all(&install_context, &resolve_options, &expanded_descriptors).await?;

        for workspace in &project.workspaces {
            let manifest_path = workspace.path
                .with_join_str("package.json");

            let manifest_content = manifest_path
                .fs_read_text_prealloc()?;

            let mut formatter
                = JsonFormatter::from(&manifest_content)?;

            for descriptor in descriptors.iter() {
                formatter.update(
                    vec!["dependencies".to_string(), descriptor.ident.to_file_string()],
                    Value::String(descriptor.range.to_file_string()),
                )?;

                formatter.update(
                    vec!["devDependencies".to_string(), descriptor.ident.to_file_string()],
                    Value::String(descriptor.range.to_file_string()),
                )?;

                formatter.update(
                    vec!["optionalDependencies".to_string(), descriptor.ident.to_file_string()],
                    Value::String(descriptor.range.to_file_string()),
                )?;
            }

            let updated_content
                = formatter.to_string();

            manifest_path
                .fs_change(&updated_content, false)?;
        }

        let mut project
            = Project::new(None).await?;

        project.run_install(RunInstallOptions {
            mode: self.mode,
            ..Default::default()
        }).await?;

        Ok(())
    }

    fn list_workspace_idents(&self, workspace: &Workspace) -> Vec<Ident> {
        let mut idents = Vec::new();

        for dependency in workspace.manifest.remote.dependencies.values() {
            idents.push(dependency.ident.clone());
        }

        for dependency in workspace.manifest.remote.optional_dependencies.values() {
            idents.push(dependency.ident.clone());
        }

        for dependency in workspace.manifest.dev_dependencies.values() {
            idents.push(dependency.ident.clone());
        }

        idents
    }
}
