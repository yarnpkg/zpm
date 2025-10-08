use clipanion::cli;
use zpm_parsers::yaml::Yaml;
use zpm_utils::IoResultExt;

use crate::{
    error::Error,
    project::Project,
};

/// Set a configuration value
#[cli::command]
#[cli::path("config", "set")]
#[cli::category("Configuration commands")]
pub struct ConfigSet {
    #[cli::option("-U,--user", default = false)]
    user: bool,

    name: zpm_parsers::Path,
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
