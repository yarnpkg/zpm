use clipanion::cli;
use convert_case::{Case, Casing};

use crate::{error::Error, project::Project, settings::{ProjectConfigType, UserConfigType}};

#[cli::command]
#[cli::path("config", "set")]
#[cli::category("Configuration commands")]
#[cli::description("Set a configuration value")]
pub struct ConfigSet {
    name: String,
    value: String,
}

impl ConfigSet {
    #[tokio::main]
    pub async fn execute(&self) -> Result<(), Error> {
        let project
            = Project::new(None).await?;

        let camel_case
            = self.name.to_case(Case::Camel);
        let snake_case
            = self.name.to_case(Case::Snake);

        if project.config.project.get(&snake_case).is_ok() {
            let hydrated_value
                = ProjectConfigType::from_file_string(&snake_case, &self.value)?;

            project.config.project.set(&snake_case, hydrated_value)?;
        } else if project.config.user.get(&snake_case).is_ok() {
            let hydrated_value
                = UserConfigType::from_file_string(&snake_case, &self.value)?;

            project.config.user.set(&snake_case, hydrated_value)?;
        } else {
            return Err(Error::ConfigKeyNotFound(camel_case));
        }

        Ok(())
    }
}
