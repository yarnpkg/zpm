use clipanion::cli;

use crate::{error::Error, project::Project, settings::ProjectConfigType};

#[cli::command]
#[cli::path("config", "set")]
#[derive(Debug)]
pub struct ConfigSet {
    name: String,
    value: String,
}

impl ConfigSet {
    #[tokio::main]
    pub async fn execute(&self) -> Result<(), Error> {
        let project
            = Project::new(None).await?;

        project.config.project
            .set(&self.name, ProjectConfigType::from_file_string(&self.name, &self.value)?)?;

        Ok(())
    }
}
