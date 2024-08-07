use clipanion::cli;

use crate::{error::Error, install::{InstallContext, InstallManager}, project};

#[cli::command(default)]
#[cli::path("install")]
pub struct Install {
}

impl Install {
    #[tokio::main()]
    pub async fn execute(&self) -> Result<(), Error> {
        let mut project
            = project::Project::new(None)?;

        let package_cache
            = project.package_cache();

        let install_context = InstallContext::default()
            .with_package_cache(Some(&package_cache))
            .with_project(Some(&project));

        let mut lockfile = project.lockfile()?;
        lockfile.forget_transient_resolutions();

        InstallManager::default()
            .with_context(install_context)
            .with_lockfile(lockfile)
            .with_roots_iter(project.workspaces.values().map(|w| w.descriptor()))
            .resolve_and_fetch().await?
            .finalize(&mut project).await?;

        Ok(())
    }
}

