use std::process::Command;

use clipanion::cli;
use zpm_utils::Path;

use crate::{errors::Error, http::fetch};

/// Update the Yarn Switch binary
///
/// This command downloads and runs the Yarn Switch installer, replacing the current Switch installation with the latest published version.
///
#[cli::command]
#[cli::path("switch", "update")]
#[cli::category("Switch commands")]
#[derive(Debug)]
pub struct UpdateCommand {
}

impl UpdateCommand {
    pub async fn execute(&self) -> Result<(), Error> {
        let (install_script_url, install_script_name) = match cfg!(windows) {
            true => ("https://repo.yarnpkg.com/install.ps1", "yarn-install-script.ps1"),
            false => ("https://repo.yarnpkg.com/install", "yarn-install-script.sh"),
        };

        let install_script
            = fetch(install_script_url).await?;

        let install_script_path
            = Path::temp_root_dir()?
                .with_join_str(install_script_name);

        install_script_path
            .fs_write(install_script)?;

        let mut command = match cfg!(windows) {
            true => {
                let mut command
                    = Command::new("powershell");

                command.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"]);
                command
            },

            false => {
                Command::new("bash")
            },
        };

        command
            .arg(install_script_path.to_path_buf())
            .status()?;

        Ok(())
    }
}
