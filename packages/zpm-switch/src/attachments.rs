use serde::{Deserialize, Serialize};
use zpm_utils::{Hash64, IoResultExt, Path, ToFileString};

use crate::errors::Error;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    pub project_cwd: Path,
    pub bin_path: Path,
}

pub fn attachments_dir() -> Result<Path, Error> {
    let attachments_dir = Path::home_dir()?
        .ok_or(Error::MissingHomeFolder)?
        .with_join_str(".yarn/switch/attachments");

    Ok(attachments_dir)
}

pub fn set_attachment(attachment: &Attachment) -> Result<(), Error> {
    let hash
        = Hash64::from_data(attachment.project_cwd.to_file_string().as_bytes());

    let attachment_path = attachments_dir()?
        .with_join_str(format!("{}.json", hash.short()));

    attachment_path
        .fs_create_parent()?
        .fs_write(sonic_rs::to_string(attachment)?)?;

    Ok(())
}

pub fn unset_attachment(project_cwd: &Path) -> Result<(), Error> {
    let hash
        = Hash64::from_data(project_cwd.to_file_string().as_bytes());

    let attachment_path = attachments_dir()?
        .with_join_str(format!("{}.json", hash.short()));

    attachment_path
        .fs_rm()?;

    Ok(())
}

pub fn get_attachment(path: &Path) -> Result<Option<Attachment>, Error> {
    let hash
        = Hash64::from_data(path.to_file_string().as_bytes());

    let attachment_path = attachments_dir()?
        .with_join_str(format!("{}.json", hash.short()));

    let attachment = attachment_path
        .fs_read_text()
        .ok_missing()?
        .and_then(|attachment| sonic_rs::from_str(&attachment).ok());

    Ok(attachment)
}
