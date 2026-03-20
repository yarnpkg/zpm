use clipanion::cli;
use zpm_utils::{DataType, Path, ToHumanString};

use crate::{cwd::get_final_cwd, errors::Error, links::{Link, LinkTarget, set_link}, manifest::find_closest_package_manager};

/// Link a local Yarn binary to the current project
#[cli::command]
#[cli::path("switch", "link")]
#[cli::category("Local Yarn development")]
#[derive(Debug)]
pub struct LinkCommand {
    path: Path,
}

impl LinkCommand {
    pub async fn execute(&self) -> Result<(), Error> {
        let lookup_path
            = get_final_cwd()?;

        let find_result
            = find_closest_package_manager(&lookup_path)?;

        let Some(detected_root_path) = find_result.detected_root_path else {
            return Err(Error::ProjectNotFound);
        };

        set_link(&Link {
            project_cwd: detected_root_path.clone(),
            link_target: LinkTarget::Local {bin_path: self.path.fs_canonicalize().await?},
        })?;

        println!(
            "Link successful; running Yarn commands in {} will now execute the binary at {}.",
            detected_root_path.to_print_string(),
            self.path.to_print_string(),
        );

        println!();

        println!(
            "Run {} to list links, and {} to remove the link from this project.",
            DataType::Code.colorize("yarn switch links"),
            DataType::Code.colorize("yarn switch unlink"),
        );

        Ok(())
    }
}
