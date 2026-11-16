use std::sync::Arc;

use clipanion::cli;
use zpm_config::Configuration;
use zpm_parsers::{DataDocument, JsonDocument, RawJsonOwnedValue};
use zpm_utils::{IoResultExt, RedactionScope, ToFileString};

use crate::{
    commands::rc_helpers,
    error::Error,
    project::Project,
};

/// Set a configuration value
///
/// This command writes a configuration setting to the project configuration file by default. Use `-H,--home` to write to the home configuration file
/// instead.
///
/// Without `--json`, the value is interpreted according to the setting type: primitive values can be passed directly, while arrays and objects are
/// parsed from JSON.
///
/// With `--json`, the value is always parsed as JSON before it is written.
///
/// Values are written in their canonical form, which `yarn config get` also prints: for example durations always get an explicit unit, using the
/// largest one that represents them exactly (`3h`, `90m`, `1d`, `0s`). The entries of settings accepting arbitrary keys (such as `npmScopes` or
/// `npmRegistries`) are created when they don't exist yet.
///
#[cli::command]
#[cli::path("config", "set")]
#[cli::category("Configuration commands")]
pub struct ConfigSet {
    /// Write to the home configuration file instead of the project file
    #[cli::option("-H,--home", default = false)]
    home: bool,

    /// Parse the value as JSON before writing it
    #[cli::option("--json", default = false)]
    json: bool,

    /// Configuration field to set
    name: zpm_parsers::Path,

    /// Value to assign to the configuration field
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
            let config = rc_helpers::load_home_config()?;
            let rc_path = rc_helpers::home_rc_path()?;
            (config, rc_path)
        } else {
            let project
                = Project::new(None).await?;

            let path
                = project.config.project_config_path.clone().unwrap();

            (project.config, path)
        };

        {
            let _scope = RedactionScope::new(false);

            // We write the value as the setting type serializes it rather than
            // as the user typed it, so that it's in its canonical form (for
            // example durations always get an explicit unit)
            let hydrate_canonical = |value_str: &str| -> Result<zpm_parsers::Value, Error> {
                let abstract_value = config.hydrate(&segments, value_str)?;
                let json_repr = abstract_value.export(true);

                Ok(match JsonDocument::hydrate_from_str::<RawJsonOwnedValue>(&json_repr) {
                    Ok(parsed) => zpm_parsers::Value::from(&parsed),
                    Err(_) => zpm_parsers::Value::String(value_str.to_string()),
                })
            };

            let value = if self.json {
                let json_value: RawJsonOwnedValue
                    = JsonDocument::hydrate_from_str(&self.value)?;

                let value
                    = zpm_parsers::Value::from(&json_value);

                // Scalars go through the setting type as well; containers are
                // written as-is (they get validated when we reload the file)
                let scalar_str = match &value {
                    zpm_parsers::Value::String(s) => Some(s.clone()),
                    zpm_parsers::Value::Number(n) => Some(n.clone()),
                    zpm_parsers::Value::Bool(b) => Some(b.to_string()),
                    _ => None,
                };

                match scalar_str.map(|s| hydrate_canonical(&s)) {
                    Some(Ok(canonical)) if !matches!(canonical, zpm_parsers::Value::Object(_) | zpm_parsers::Value::Array(_)) => canonical,
                    _ => value,
                }
            } else {
                hydrate_canonical(&self.value)?
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

            Configuration::validate(&document_path, &updated_document)
                .map_err(|e| Error::ConfigurationParseError(Arc::new(e)))?;

            document_path
                .fs_change(&updated_document, false)?;
        }

        let config = if self.home {
            rc_helpers::load_home_config()?
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
}
