use clipanion::cli;

use crate::{error::Error, project::{self, RunInstallOptions}};

#[cli::command(default)]
#[cli::path("install")]
pub struct Install {
    #[cli::option("--check-resolutions", default = false)]
    check_resolutions: bool,

    #[cli::option("--immutable", default = false)]
    immutable: bool,

    #[cli::option("--immutable-cache", default = false)]
    immutable_cache: bool,

    #[cli::option("--refresh-lockfile", default = false)]
    refresh_lockfile: bool,
}

impl Install {
    #[tokio::main()]
    pub async fn execute(&self) -> Result<(), Error> {
        let mut project
            = project::Project::new(None).await?;

        if self.immutable {
            project.config.project.enable_immutable_installs.value = true;
        }

        if self.immutable_cache {
            project.config.project.enable_immutable_cache.value = true;
        }

        project.run_install(RunInstallOptions {
            check_resolutions: self.check_resolutions,
            refresh_lockfile: self.refresh_lockfile,
        }).await?;

        Ok(())
    }
}

