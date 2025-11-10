use clipanion::cli;
use zpm_utils::set_redacted;

use crate::{error::Error, project::Project};

/// Get a configuration value
///
/// This command will print a configuration setting.
///
/// Secrets (such as tokens) will be redacted from the output by default. If this behavior isn't desired, set the `--no-redacted` to get the
/// untransformed value.
///
#[cli::command]
#[cli::path("config", "get")]
#[cli::category("Configuration commands")]
pub struct ConfigGet {
    /// Format the output as a JSON value
    #[cli::option("--json", default = false)]
    json: bool,

    /// Redact sensitive values
    #[cli::option("--redacted", default = true)]
    redacted: bool,

    /// The name of the configuration field to retrieve
    name: zpm_parsers::path::Path,
}

impl ConfigGet {
    pub async fn execute(&self) -> Result<(), Error> {
        let project
            = Project::new(None).await?;

        set_redacted(self.redacted);

        let segments
            = self.name.segments()
                .iter()
                .map(|v| v.as_str())
                .collect::<Vec<_>>();

        let entry
            = project.config.get(&segments)?;

        println!("{}", entry.value.export(self.json));

        Ok(())
    }
}
