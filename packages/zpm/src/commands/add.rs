use clipanion::cli;
use zpm_semver::RangeKind;
use zpm_utils::FromFileString;

use crate::{error::Error, install::InstallContext, primitives::{loose_descriptor, range::SemverPeerRange, LooseDescriptor, PeerRange}, project};

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
            = project.package_cache();

        let install_context = InstallContext::default()
            .with_package_cache(Some(&package_cache))
            .with_project(Some(&project));

        let descriptors
            = LooseDescriptor::resolve_all(&install_context, &resolve_options, &self.descriptors).await?;

        let active_workspace
            = project.active_workspace_mut()?;

        for descriptor in &descriptors {
            let mut dev = self.dev;
            let mut optional = self.optional;
            let mut prod = false;

            // FUTURE: We probably should use a similar logic as for the dev/optional/prod flags, where
            // we check if the dependency is already listed in the manifest, and if so, we set the flag
            // to true. But since we always set peer dependencies to `*` at the moment, it's not very
            // useful. In the future I'd like to instead set it to the current major/rc.
            let peer = self.peer;

            if !dev && !optional && !peer {
                if active_workspace.manifest.dev_dependencies.contains_key(&descriptor.ident) {
                    dev = true;
                }
                if active_workspace.manifest.remote.optional_dependencies.contains_key(&descriptor.ident) {
                    optional = true;
                }
                if !dev && !optional && !peer {
                    prod = true;
                }
            }

            if dev && active_workspace.manifest.remote.dependencies.contains_key(&descriptor.ident) {
                return Err(Error::ConflictingOptions(format!("{} is already listed as a regular dependency of this workspace", descriptor.ident)));
            }

            if optional && active_workspace.manifest.remote.dependencies.contains_key(&descriptor.ident) {
                return Err(Error::ConflictingOptions(format!("{} is already listed as an regular dependency of this workspace", descriptor.ident)));
            }

            if peer && active_workspace.manifest.remote.dependencies.contains_key(&descriptor.ident) {
                return Err(Error::ConflictingOptions(format!("{} is already listed as a regular dependency of this workspace", descriptor.ident)));
            }

            if prod && active_workspace.manifest.remote.peer_dependencies.contains_key(&descriptor.ident) {
                return Err(Error::ConflictingOptions(format!("{} is already listed as a peer dependency of this workspace", descriptor.ident)));
            }

            if dev {
                active_workspace.manifest.dev_dependencies.insert(descriptor.ident.clone(), descriptor.clone());
            }

            if optional {
                active_workspace.manifest.remote.optional_dependencies.insert(descriptor.ident.clone(), descriptor.clone());
            }

            if peer {
                active_workspace.manifest.remote.peer_dependencies.insert(descriptor.ident.clone(), PeerRange::Semver(SemverPeerRange {range: zpm_semver::Range::from_file_string("*").unwrap()}));
            }

            if prod {
                active_workspace.manifest.remote.dependencies.insert(descriptor.ident.clone(), descriptor.clone());
            }
        }    

        active_workspace.write_manifest()?;

        project.run_install().await?;

        Ok(())
    }
}
