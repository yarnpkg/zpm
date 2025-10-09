use clipanion::cli;

use crate::{error::Error, project::Project};

/// Get a configuration value
#[cli::command]
#[cli::path("config", "get")]
#[cli::category("Configuration commands")]
pub struct ConfigGet {
    #[cli::option("--json", default = false)]
    json: bool,

    name: zpm_parsers::path::Path,
}

impl ConfigGet {
    pub async fn execute(&self) -> Result<(), Error> {
        let project
            = Project::new(None).await?;

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
