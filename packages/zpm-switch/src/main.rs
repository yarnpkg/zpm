use std::process::{Command, ExitCode, ExitStatus, Stdio};

use clipanion::{cli, prelude::*};
use install::install_package_manager;
use manifest::{find_closest_package_manager, validate_package_manager, PackageManagerField, PackageManagerReference};
use yarn::get_default_yarn_version;
use zpm_utils::{ExplicitPath, Path};

mod errors;
mod http;
mod install;
mod manifest;
mod yarn;

use crate::errors::Error;

#[cli::command(default, proxy)]
#[derive(Debug)]
pub struct ProxyCommand {
    #[cli::positional(is_prefix = true)]
    cwd: Option<ExplicitPath>,

    args: Vec<String>,
}

impl ProxyCommand {
    async fn execute(&self) -> Result<ExitStatus, Error> {
        let lookup_path = match &self.cwd {
            Some(cwd) => Path::current_dir()?.with_join(&cwd.raw_path.path),
            None => Path::current_dir()?,
        };

        let find_result
            = find_closest_package_manager(&lookup_path)?;

        if let Some(detected_root_path) = find_result.detected_root_path {
            std::env::set_var("YARNSW_DETECTED_ROOT", detected_root_path.to_string());
        }

        let reference = match find_result.detected_package_manager {
            Some(package_manager) => validate_package_manager(package_manager, "yarn"),
            None => get_default_yarn_version(Some("classic")).await,
        }?;

        ExplicitCommand::run(&reference, &self.args).await
    }
}

#[cli::command(proxy)]
#[cli::path("init")]
#[derive(Debug)]
pub struct InitCommand {
    #[cli::positional(is_prefix = true)]
    cwd: Option<ExplicitPath>,

    args: Vec<String>,
}

impl InitCommand {
    async fn execute(&self) -> Result<ExitStatus, Error> {
        let lookup_path = match &self.cwd {
            Some(cwd) => Path::current_dir()?.with_join(&cwd.raw_path.path),
            None => Path::current_dir()?,
        };

        let find_result
            = find_closest_package_manager(&lookup_path)?;

        if let Some(detected_root_path) = find_result.detected_root_path {
            std::env::set_var("YARNSW_DETECTED_ROOT", detected_root_path.to_string());
        }

        let reference = match find_result.detected_package_manager {
            Some(package_manager) => validate_package_manager(package_manager, "yarn"),
            None => get_default_yarn_version(None).await,
        }?;

        let mut args = vec!["init".to_string()];
        args.extend_from_slice(&self.args);

        ExplicitCommand::run(&reference, &args).await
    }
}

#[cli::command(default)]
#[cli::path("switch", "postinstall")]
#[derive(Debug)]
pub struct PostinstallCommand {
}

impl PostinstallCommand {
    async fn execute(&self) -> Result<ExitCode, Error> {
        Ok(ExitCode::SUCCESS)
    }
}

#[cli::command(default, proxy)]
#[cli::path("switch")]
#[derive(Debug)]
pub struct ExplicitCommand {
    #[cli::positional(is_prefix = true)]
    cwd: Option<ExplicitPath>,

    package_manager: PackageManagerField,
    args: Vec<String>,
}

impl ExplicitCommand {
    async fn run(reference: &PackageManagerReference, args: &[String]) -> Result<ExitStatus, Error> {
        let mut binary = match reference {
            PackageManagerReference::Version(params)
                => install_package_manager(params).await?,
    
            PackageManagerReference::Local(params)
                => Command::new(params.path.to_path_buf()),
        };

        binary.stdout(Stdio::inherit());
        binary.args(args);
    
        Ok(binary.status()?)
    }

    pub async fn execute(&self) -> Result<ExitStatus, Error> {
        let lookup_path = match &self.cwd {
            Some(cwd) => Path::current_dir()?.with_join(&cwd.raw_path.path),
            None => Path::current_dir()?,
        };

        let find_result
            = find_closest_package_manager(&lookup_path)?;

        if let Some(detected_root_path) = find_result.detected_root_path {
            std::env::set_var("YARNSW_DETECTED_ROOT", detected_root_path.to_string());
        }

        ExplicitCommand::run(&self.package_manager.reference, &self.args).await
    }
}

clipanion::program_async!(SwitchExecCli, [
    PostinstallCommand,
    ExplicitCommand,
    ProxyCommand,
    InitCommand,
]);

#[tokio::main()]
async fn main() -> ExitCode {
    let self_path = Path::current_exe()
        .unwrap()
        .to_string();

    std::env::set_var("YARNSW_EXEC_PATH", self_path);

    SwitchExecCli::run_default().await
}
