use clipanion::cli;
use zpm_utils::Path;

use crate::{links::unset_link, cwd::get_final_cwd, errors::Error, manifest::find_closest_package_manager};

/// Remove a Yarn Switch link from a project
///
/// This command removes a local binary link or migration link so the project goes back to the version declared in `packageManager`.
///
#[cli::command]
#[cli::path("switch", "unlink")]
#[cli::category("Local Yarn development")]
#[derive(Debug)]
pub struct UnlinkCommand {
    /// Project path whose link should be removed; defaults to the current project
    project_cwd: Option<Path>,
}

impl UnlinkCommand {
    pub async fn execute(&self) -> Result<(), Error> {
        let lookup_path
            = self.project_cwd
                .clone()
                .map_or_else(get_final_cwd, Ok)?;

        let find_result
            = find_closest_package_manager(&lookup_path)?;

        let Some(detected_root_path) = find_result.detected_root_path else {
            return Err(Error::ProjectNotFound);
        };

        unset_link(&detected_root_path)?;

        Ok(())
    }
}
