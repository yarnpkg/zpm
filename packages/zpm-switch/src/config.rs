use serde::Deserialize;
use zpm_semver::{Range, Version};
use zpm_utils::{IoResultExt, Path};

use crate::errors::Error;

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct PartialRcSettings {
    #[serde(default)]
    switch_version_requirement: Option<Range>,
}

fn load_switch_version_requirement() -> Result<Option<Range>, Error> {
    let home_dir
        = Path::home_dir()?
            .ok_or(Error::MissingHomeFolder)?;

    let rc_filename
        = std::env::var("YARN_RC_FILENAME")
            .unwrap_or_else(|_| ".yarnrc.yml".to_string());

    let rc_path
        = home_dir.with_join_str(&rc_filename);

    let text
        = rc_path.fs_read_text()
            .ok_missing()?;

    let Some(text) = text else {
        return Ok(None);
    };

    if text.is_empty() {
        return Ok(None);
    }

    let partial: PartialRcSettings
        = serde_yaml::from_str(&text)?;

    Ok(partial.switch_version_requirement)
}

pub fn validate_yarn_version(version: &Version) -> Result<(), Error> {
    let Some(requirement) = load_switch_version_requirement()? else {
        return Ok(());
    };

    if !requirement.check_ignore_rc(version) {
        return Err(Error::SwitchVersionMismatch {
            requirement,
            actual: version.clone(),
        });
    }

    Ok(())
}
