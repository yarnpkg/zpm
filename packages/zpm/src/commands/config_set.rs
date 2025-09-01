use clipanion::cli;

use crate::{
    error::Error,
    project::Project,
};

#[cli::command]
#[cli::path("config", "set")]
#[cli::category("Configuration commands")]
#[cli::description("Set a configuration value")]
pub struct ConfigSet {
    #[cli::option("-U,--user")]
    user: bool,

    name: zpm_parsers::path::Path,
    value: String,
}

impl ConfigSet {
    #[tokio::main]
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

        Ok(())
    }
}
