use clipanion::cli;

use crate::{attachments::unset_attachment, cwd::get_final_cwd, errors::Error, manifest::find_closest_package_manager};

#[cli::command]
#[cli::path("switch", "detach")]
#[derive(Debug)]
pub struct DetachCommand {
}

impl DetachCommand {
    pub async fn execute(&self) -> Result<(), Error> {
        let lookup_path
            = get_final_cwd()?;

        let find_result
            = find_closest_package_manager(&lookup_path)?;

        let Some(detected_root_path) = find_result.detected_root_path else {
            return Err(Error::ProjectNotFound);
        };

        unset_attachment(&detected_root_path)?;

        Ok(())
    }
}
