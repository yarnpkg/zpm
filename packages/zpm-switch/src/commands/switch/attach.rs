use clipanion::cli;
use zpm_utils::{Path, ToHumanString};

use crate::{attachments::{set_attachment, Attachment}, cwd::get_final_cwd, errors::Error, manifest::find_closest_package_manager};

#[cli::command]
#[cli::path("switch", "attach")]
#[derive(Debug)]
pub struct AttachCommand {
    path: Path,
}

impl AttachCommand {
    pub async fn execute(&self) -> Result<(), Error> {
        let lookup_path
            = get_final_cwd()?;

        let find_result
            = find_closest_package_manager(&lookup_path)?;

        let Some(detected_root_path) = find_result.detected_root_path else {
            return Err(Error::ProjectNotFound);
        };

        set_attachment(&Attachment {
            project_cwd: detected_root_path.clone(),
            bin_path: self.path.fs_canonicalize()?,
        })?;

        println!("Attached {} to {}", self.path.to_print_string(), detected_root_path.to_print_string());

        Ok(())
    }
}
