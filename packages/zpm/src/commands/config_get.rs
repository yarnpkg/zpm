use clipanion::cli;
use zpm_utils::set_redacted;

use crate::{error::Error, project::Project};

/// Print a configuration value
///
/// This command prints a single configuration setting as resolved for the current project.
///
/// Secrets such as tokens are redacted from the output by default. Use `--no-redacted` only when you need the untransformed value.
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

    /// Configuration field to retrieve
    name: zpm_parsers::Path,
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
