use clipanion::cli;
use zpm_parsers::yaml::Yaml;
use zpm_utils::IoResultExt;

use crate::{
    error::Error,
    project::Project,
};

/// Set a configuration value
///
/// This command will set a configuration setting, by default in the project configuration file unless the `-U,--user` flag is set.
///
/// The new value will be hydrated depending on the type of the field being set: primitives such as string will be hydrated directly, while complex
/// types such as arrays and objects will be hydrated through JSON.
///
#[cli::command]
#[cli::path("config", "set")]
#[cli::category("Configuration commands")]
pub struct ConfigSet {
    // If set, the configuration will be set in the user configuration file
    #[cli::option("-U,--user", default = false)]
    user: bool,

    /// The name of the configuration value to set
    name: zpm_parsers::Path,

    /// The value to set the configuration value to
    value: String,
}

impl ConfigSet {
    pub async fn execute(&self) -> Result<(), Error> {
        let project
            = Project::new(None).await?;

        let segments
            = self.name.segments()
                .iter()
                .map(|v| v.as_str())
                .collect::<Vec<_>>();

        let value
            = project.config.hydrate(&segments, &self.value)?;

        let document_path = match self.user {
            true => project.config.user_config_path.as_ref().unwrap(),
            false => project.config.project_config_path.as_ref().unwrap(),
        };

        let document = document_path
            .fs_read_text()
            .ok_missing()?
            .unwrap_or_default();

        let updated_document = Yaml::update_document_field(
            &document,
            self.name.clone(),
            zpm_parsers::Value::Raw(value.export(true)),
        )?;

        document_path
            .fs_change(&updated_document, false)?;

        Ok(())
    }
}
