use clipanion::cli;
use convert_case::{Case, Casing};

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

        let snake_case_name
            = &self.name.to_case(Case::Snake);

        let hydrated_value
            = ProjectConfigType::from_file_string(snake_case_name, &self.value)?;

        project.config.project
            .set(snake_case_name, hydrated_value)?;

        Ok(())
    }
}
