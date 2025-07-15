use std::process::ExitStatus;

use clipanion::cli;
use zpm_utils::ToFileString;

use crate::{cwd::{get_final_cwd, restore_args}, errors::Error, manifest::{find_closest_package_manager, validate_package_manager}, yarn::get_default_yarn_version, yarn_enums::ReleaseLine};

use super::switch::explicit::ExplicitCommand;

#[cli::command(default, proxy)]
#[derive(Debug)]
pub struct ProxyCommand {
    args: Vec<String>,
}

impl ProxyCommand {
    pub async fn execute(&self) -> Result<ExitStatus, Error> {
        let lookup_path
            = get_final_cwd()?;

        let find_result
            = find_closest_package_manager(&lookup_path)?;

        if let Some(detected_root_path) = find_result.detected_root_path {
            std::env::set_var("YARNSW_DETECTED_ROOT", detected_root_path.to_file_string());
        }

        let reference = match find_result.detected_package_manager {
            Some(package_manager) => {
                validate_package_manager(package_manager, "yarn")
            },

            None => {
                get_default_yarn_version(Some(ReleaseLine::Classic)).await
            },
        }?;

        let mut args
            = self.args.clone();

        // Don't forget to add back the cwd parameter that was removed earlier on!
        restore_args(&mut args);

        ExplicitCommand::run(&reference, &args).await
    }
}
