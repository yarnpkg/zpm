use std::sync::Arc;

use clipanion::cli;
use zpm_config::Configuration;
use zpm_parsers::{DataDocument, JsonDocument, Value};
use zpm_utils::{IoResultExt, ToFileString};

use crate::{
    commands::rc_helpers,
    error::Error,
    project::Project,
};

fn path_exists<'a>(value: &serde_json::Value, mut segments: impl Iterator<Item = &'a str>) -> bool {
    match segments.next() {
        None => true,
        Some(segment) => {
            match value {
                serde_json::Value::Object(map) => match map.get(segment) {
                    Some(child) => path_exists(child, segments),
                    None => false,
                },
                _ => false,
            }
        }
    }
}

/// Remove a configuration value
///
/// This command removes a configuration setting from the project configuration file by default. Use `-H,--home` to remove it from the home
/// configuration file instead.
///
#[cli::command]
#[cli::path("config", "unset")]
#[cli::category("Configuration commands")]
pub struct ConfigUnset {
    /// Remove the value from the home configuration file instead of the project file
    #[cli::option("-H,--home", default = false)]
    home: bool,

    /// Configuration field to remove
    name: zpm_parsers::Path,
}

impl ConfigUnset {
    pub async fn execute(&self) -> Result<(), Error> {
        let document_path = if self.home {
            rc_helpers::home_rc_path()?
        } else {
            let project = Project::new(None).await?;

            project.config.project_config_path
                .clone()
                .ok_or(Error::HomeDirectoryNotFound)?
        };

        let document
            = document_path
                .fs_read_text()
                .ok_missing()?
                .unwrap_or_default();

        let already_present = if document.trim().is_empty() {
            false
        } else {
            let parsed: serde_json::Value = JsonDocument::hydrate_from_str(&document)
                .or_else(|_| zpm_parsers::YamlDocument::hydrate_from_str(&document))?;

            path_exists(&parsed, self.name.segments().iter().map(|s| s.as_str()))
        };

        if !already_present {
            println!("Configuration doesn't contain setting {}; there is nothing to unset", self.name.to_file_string());
            return Ok(());
        }

        let updated_document = DataDocument::update_document_field(
            &document,
            self.name.clone(),
            Value::Undefined,
        )?;

        Configuration::validate(&updated_document)
            .map_err(|e| Error::ConfigurationParseError(Arc::new(e)))?;

        document_path
            .fs_change(&updated_document, false)?;

        let _config: Configuration = if self.home {
            rc_helpers::load_home_config()?
        } else {
            Project::new(None).await?.config
        };

        println!("Successfully unset {}", self.name.to_file_string());

        Ok(())
    }
}
