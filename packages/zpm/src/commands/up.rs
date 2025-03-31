use std::collections::BTreeSet;

use clipanion::cli;
use zpm_semver::RangeKind;

use crate::{error::Error, install::InstallContext, primitives::{loose_descriptor, Ident, LooseDescriptor}, project::{self, RunInstallOptions, Workspace}};

#[cli::command]
#[cli::path("up")]
pub struct Up {
    #[cli::option("-F,--fixed")]
    fixed: bool,

    #[cli::option("-E,--exact")]
    exact: bool,

    #[cli::option("-T,--tilde")]
    tilde: bool,

    #[cli::option("-C,--caret")]
    caret: bool,

    // ---

    descriptors: Vec<LooseDescriptor>,
}

impl Up {
    #[tokio::main()]
    pub async fn execute(&self) -> Result<(), Error> {
        let mut project
            = project::Project::new(None).await?;

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
            project.config.project.default_semver_range_prefix.value
        };

        let resolve_options = loose_descriptor::ResolveOptions {
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

        for workspace in project.workspaces.iter_mut() {
            for descriptor in descriptors.iter() {
                if workspace.manifest.remote.dependencies.contains_key(&descriptor.ident) {
                    workspace.manifest.remote.dependencies.insert(descriptor.ident.clone(), descriptor.clone());
                }

                if workspace.manifest.remote.optional_dependencies.contains_key(&descriptor.ident) {
                    workspace.manifest.remote.optional_dependencies.insert(descriptor.ident.clone(), descriptor.clone());
                }

                if workspace.manifest.dev_dependencies.contains_key(&descriptor.ident) {
                    workspace.manifest.dev_dependencies.insert(descriptor.ident.clone(), descriptor.clone());
                }
            }

            workspace.write_manifest()?;
        }

        project.run_install(RunInstallOptions {
            check_resolutions: false,
            refresh_lockfile: false,
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
