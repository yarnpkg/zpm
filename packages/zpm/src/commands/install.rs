use clipanion::cli;

use crate::{error::Error, project::{self, InstallMode, RunInstallOptions}};

#[cli::command(default)]
#[cli::path("install")]
#[cli::category("Dependency management")]
#[cli::description("Install dependencies")]
pub struct Install {
    #[cli::option("--check-resolutions", default = false)]
    check_resolutions: bool,

    #[cli::option("--immutable", default = false)]
    immutable: bool,

    #[cli::option("--immutable-cache", default = false)]
    immutable_cache: bool,

    #[cli::option("--check-checksums", default = false)]
    check_checksums: bool,

    #[cli::option("--refresh-lockfile", default = false)]
    refresh_lockfile: bool,

    #[cli::option("--inline-builds", default = false)]
    inline_builds: bool,

    #[cli::option("--mode")]
    mode: Option<InstallMode>,
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

        if self.inline_builds {
            project.config.project.enable_inline_builds.value = true;
        }

        project.run_install(RunInstallOptions {
            check_checksums: self.check_checksums,
            check_resolutions: self.check_resolutions,
            refresh_lockfile: self.refresh_lockfile,
            mode: self.mode,
            ..Default::default()
        }).await?;

        Ok(())
    }
}

