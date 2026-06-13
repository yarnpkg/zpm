use clipanion::cli;
use zpm_config::Settings;
use zpm_utils::set_redacted;

use crate::{error::Error, project::Project};

/// List the effective configuration values
///
/// This command prints the configuration visible from the current project, including each setting's resolved value and source. Secrets are redacted
/// by default; use `--no-redacted` only when you need to inspect the stored value.
#[cli::command]
#[cli::path("config")]
#[cli::path("config", "get")]
#[cli::category("Configuration commands")]
pub struct Config {
    /// Format the output as an NDJSON stream
    #[cli::option("--json", default = false)]
    json: bool,

    /// Show sensitive values instead of redacting them
    #[cli::option("--no-redacted")]
    no_redacted: Option<bool>,
}

impl Config {
    pub async fn execute(&self) -> Result<(), Error> {
        let project
            = Project::new(None).await?;

        // `--no-redacted` (Some(true)) is the only case where the user
        // wants secrets revealed. Everything else (explicit
        // `--no-redacted=false`, or the flag unset) keeps the default
        // redaction on.
        set_redacted(self.no_redacted != Some(true));

        if !self.json {
            let tree
                = project.config.tree_node();

            print!("{}", tree.to_string());

            return Ok(());
        }

        for name in Settings::setting_names() {
            let entry = match project.config.get(&[name]) {
                Ok(entry) => entry,
                Err(_) => continue,
            };

            let json_str = entry.value.export(true);
            let value: serde_json::Value = serde_json::from_str(&json_str)
                .unwrap_or_else(|_| serde_json::Value::String(json_str.clone()));

            let line = serde_json::json!({
                "key": name,
                "effective": value,
                "source": entry.source.label(),
            });

            println!("{}", serde_json::to_string(&line).unwrap());
        }

        Ok(())
    }
}
