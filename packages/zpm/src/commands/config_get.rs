use clipanion::cli;

use crate::{error::Error, project::Project};

#[cli::command]
#[cli::path("config", "get")]
#[cli::category("Configuration commands")]
#[cli::description("Get a configuration value")]
pub struct ConfigGet {
    name: zpm_parsers::path::Path,
}

impl ConfigGet {
    #[tokio::main]
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

        println!("{}", entry.value.to_print_string());

        Ok(())
    }
}
