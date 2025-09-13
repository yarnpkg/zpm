use clipanion::cli;
use zpm_utils::{Path, ToHumanString};

use crate::{links::{set_link, Link}, cwd::get_final_cwd, errors::Error, manifest::find_closest_package_manager};

#[cli::command]
#[cli::path("switch", "link")]
#[cli::category("Local Yarn development")]
#[cli::description("Link a local Yarn binary to the current project")]
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
            bin_path: self.path.fs_canonicalize()?,
        })?;

        println!("Linked {} to {}", self.path.to_print_string(), detected_root_path.to_print_string());

        Ok(())
    }
}
