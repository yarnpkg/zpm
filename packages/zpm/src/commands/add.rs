use clipanion::cli;
use zpm_semver::RangeKind;

use crate::{error::Error, install::InstallContext, primitives::{loose_descriptor, LooseDescriptor}, project};

#[cli::command]
#[cli::path("add")]
pub struct Add {
    #[cli::option("-F,--fixed")]
    fixed: bool,

    #[cli::option("-E,--exact")]
    exact: bool,

    #[cli::option("-T,--tilde")]
    tilde: bool,

    #[cli::option("-C,--caret")]
    caret: bool,

    // ---

    #[cli::option("-P,--peer")]
    peer: bool,

    #[cli::option("-D,--dev")]
    dev: bool,

    #[cli::option("-O,--optional")]
    optional: bool,

    #[cli::option("--prefer-dev")]
    prefer_dev: bool,

    // ---

    descriptors: Vec<LooseDescriptor>,
}

impl Add {
    #[tokio::main()]
    pub async fn execute(&self) -> Result<(), Error> {
        let mut project
            = project::Project::new(None).await?;

        println!("descriptors: {:#?}", self.descriptors);

        let range_kind = if self.fixed {
            RangeKind::Exact
        } else if self.exact {
            RangeKind::Exact
        } else if self.tilde {
            RangeKind::Tilde
        } else {
            RangeKind::Caret
        };

        let resolve_options = loose_descriptor::ResolveOptions {
            range_kind,
            resolve_tags: !self.fixed,
        };

        let package_cache
            = project.package_cache();

        let install_context = InstallContext::default()
            .with_package_cache(Some(&package_cache))
            .with_project(Some(&project));

        let descriptors
            = LooseDescriptor::resolve_all(&install_context, &resolve_options, &self.descriptors).await?;

        let active_workspace
            = project.active_workspace_mut()?;

        if self.peer {
            for descriptor in &descriptors {
                active_workspace.manifest.remote.peer_dependencies.insert(descriptor.ident.clone(), descriptor.range.to_peer_range()?);
            }
        } else {
            let target = if self.dev {
                &mut active_workspace.manifest.dev_dependencies
            } else if self.optional {
                &mut active_workspace.manifest.remote.optional_dependencies
            } else {
                &mut active_workspace.manifest.remote.dependencies
            };

            for descriptor in &descriptors {
                target.insert(descriptor.ident.clone(), descriptor.clone());
            }    
        }

        active_workspace.write_manifest()?;

        project.run_install().await?;

        Ok(())
    }
}
