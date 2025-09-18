use std::{process::{Command, ExitStatus, Stdio}, sync::Arc};

use clipanion::cli;
use zpm_utils::ToFileString;

use crate::{cwd::{get_fake_cwd, get_final_cwd}, errors::Error, install::install_package_manager, manifest::{find_closest_package_manager, PackageManagerReference, VersionPackageManagerReference}, yarn::resolve_selector, yarn_enums::Selector};

#[cli::command(proxy)]
#[cli::path("switch")]
#[cli::category("General commands")]
#[cli::description("Call a custom Yarn binary for the current project")]
#[derive(Debug)]
pub struct ExplicitCommand {
    selector: Selector,
    args: Vec<String>,
}

impl ExplicitCommand {
    pub async fn run(reference: &PackageManagerReference, args: &[String]) -> Result<ExitStatus, Error> {
        let mut binary = match reference {
            PackageManagerReference::Version(params)
                => install_package_manager(params).await?,

            PackageManagerReference::Local(params)
                => Command::new(params.path.to_path_buf()),
        };

        binary.stdout(Stdio::inherit());
        binary.args(args);

        let exit_code
            = binary.status()
                .map_err(|err| Error::FailedToExecuteBinary(Arc::new(err)))?;

        Ok(exit_code)
    }

    pub async fn execute(&self) -> Result<ExitStatus, Error> {
        let lookup_path
            = get_final_cwd()?;

        let find_result
            = find_closest_package_manager(&lookup_path)?;

        if let Some(detected_root_path) = find_result.detected_root_path {
            std::env::set_var("YARNSW_DETECTED_ROOT", detected_root_path.to_file_string());
        }

        let mut args
            = self.args.clone();

        // Don't forget to add back the cwd parameter that was removed earlier on!
        if let Some(cwd) = get_fake_cwd() {
            args.insert(0, cwd.to_file_string());
        }

        let version
            = resolve_selector(&self.selector).await?;

        let reference
            = VersionPackageManagerReference {version};

        ExplicitCommand::run(&reference.into(), &args).await
    }
}
