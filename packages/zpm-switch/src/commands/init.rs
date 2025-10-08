use std::process::ExitStatus;

use clipanion::cli;
use zpm_utils::ToFileString;

use crate::{cwd::get_final_cwd, errors::Error, manifest::{find_closest_package_manager, validate_package_manager}, yarn::get_default_yarn_version};

use super::switch::explicit::ExplicitCommand;

/// Initialize a new Yarn project
#[cli::command(proxy)]
#[cli::path("init")]
#[derive(Debug)]
pub struct InitCommand {
    args: Vec<String>,
}

impl InitCommand {
    pub async fn execute(&self) -> Result<ExitStatus, Error> {
        let lookup_path
            = get_final_cwd()?;

        let find_result
            = find_closest_package_manager(&lookup_path)?;

        if let Some(detected_root_path) = find_result.detected_root_path {
            std::env::set_var("YARNSW_DETECTED_ROOT", detected_root_path.to_file_string());
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
