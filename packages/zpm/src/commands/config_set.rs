use std::sync::Arc;

use clipanion::cli;
use zpm_config::{Configuration, ConfigurationContext};
use zpm_parsers::{DataDocument, JsonDocument, RawJsonOwnedValue};
use zpm_utils::{IoResultExt, LastModifiedAt, Path, ToFileString, ToHumanString, set_redacted};

use crate::{
    error::Error,
    project::Project,
};

/// Set a configuration value
///
/// This command will set a configuration setting, by default in the project configuration file unless the `-H,--home` flag is set, in which case it
/// will be set in the home configuration file.
///
/// When used without the `--json` flag, the new value will be hydrated depending on the type of the field being set: primitives such as strings will
/// be hydrated directly, while complex types such as arrays and objects will be hydrated through JSON.
///
/// When used with the `--json` flag, the new value will always be parsed as JSON before being written to the configuration file.
///
#[cli::command]
#[cli::path("config", "set")]
#[cli::category("Configuration commands")]
pub struct ConfigSet {
    /// If set, the configuration will be set in the home configuration file
    #[cli::option("-H,--home", default = false)]
    home: bool,

    /// Set complex configuration settings to JSON values
    #[cli::option("--json", default = false)]
    json: bool,

    /// The name of the configuration value to set
    name: zpm_parsers::Path,

    /// The value to set the configuration value to
    value: String,
}

impl ConfigSet {
    pub async fn execute(&self) -> Result<(), Error> {
        let segments
            = self.name.segments()
                .iter()
                .map(|v| v.as_str())
                .collect::<Vec<_>>();

        let (config, document_path) = if self.home {
            let config
                = Self::load_home_config()?;

            let user_cwd = Path::home_dir()?
                .ok_or(Error::HomeDirectoryNotFound)?;

            let rc_filename = std::env::var("YARN_RC_FILENAME")
                .unwrap_or_else(|_| ".yarnrc.yml".to_string());

            (config, user_cwd.with_join_str(&rc_filename))
        } else {
            let project
                = Project::new(None).await?;

            let path
                = project.config.project_config_path.clone().unwrap();

            (project.config, path)
        };

        let value = if self.json {
            let json_value: RawJsonOwnedValue
                = JsonDocument::hydrate_from_str(&self.value)?;

            zpm_parsers::Value::from(&json_value)
        } else {
            config.hydrate(&segments, &self.value)?;
            zpm_parsers::Value::String(self.value.clone())
        };

        let document
            = document_path
                .fs_read_text()
                .ok_missing()?
                .unwrap_or_default();

        let updated_document = DataDocument::update_document_field(
            &document,
            self.name.clone(),
            value,
        )?;

        document_path
            .fs_change(&updated_document, false)?;

        set_redacted(true);

        let config = if self.home {
            Self::load_home_config()?
        } else {
            Project::new(None).await?.config
        };

        let entry
            = config.get(&segments)?;

        let is_container
            = entry.value.is_container();
        let json_str
            = entry.value.export(true);

        let display_value = if is_container {
            let yaml_value: serde_yaml::Value
                = JsonDocument::hydrate_from_str(&json_str)?;

            serde_yaml::to_string(&yaml_value)
                .map_err(|e| Error::ConfigurationParseError(Arc::new(e)))?
                .trim_end()
                .to_string()
        } else {
            json_str
        };

        println!("Successfully set {} to {}", self.name.to_file_string(), display_value);

        Ok(())
    }

    fn load_home_config() -> Result<Configuration, Error> {
        let user_cwd
            = Path::home_dir()?
                .ok_or(Error::HomeDirectoryNotFound)?;

        let context = ConfigurationContext {
            env: std::env::vars().collect(),
            user_cwd: Some(user_cwd),
            project_cwd: None,
            package_cwd: None,
        };

        let mut last_modified_at
            = LastModifiedAt::new();

        Configuration::load(&context, &mut last_modified_at)
            .map_err(|e| Error::ConfigurationParseError(Arc::new(e)))
    }
}
