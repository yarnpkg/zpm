use std::process::Command;

use clipanion::cli;
use zpm_utils::Path;

use crate::{errors::Error, http::fetch};

#[cli::command]
#[cli::path("switch", "update")]
#[cli::category("Switch commands")]
#[cli::description("Update the Yarn Switch binary to the latest version")]
#[derive(Debug)]
pub struct UpdateCommand {
}

impl UpdateCommand {
    pub async fn execute(&self) -> Result<(), Error> {
        let install_script_url
            = "https://repo.yarnpkg.com/install";

        let install_script
            = fetch(install_script_url).await?;

        let install_script_path
            = Path::temp_root_dir()?
                .with_join_str("yarn-install-script.sh");

        install_script_path
            .fs_write(install_script)?;

        Command::new("bash")
            .arg(install_script_path.to_path_buf())
            .status()?;

        Ok(())
    }
}
