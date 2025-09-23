use clipanion::cli;
use zpm_config::Source;
use zpm_utils::ResultExt;

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

    #[cli::option("--mode")]
    mode: Option<InstallMode>,
}

impl Install {
    #[tokio::main()]
    pub async fn execute(&self) -> Result<(), Error> {
        let mut project
            = project::Project::new(None).await?;

        if self.immutable {
            project.config.settings.enable_immutable_installs.value = true;
            project.config.settings.enable_immutable_installs.source = Source::Cli;
        }

        if self.immutable_cache {
            project.config.settings.enable_immutable_cache.value = true;
            project.config.settings.enable_immutable_cache.source = Source::Cli;
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
