use clipanion::cli;
use convert_case::{Case, Casing};
use zpm_utils::ToHumanString;

use crate::{error::Error, project::Project};

#[cli::command]
#[cli::path("config", "get")]
#[cli::category("Configuration commands")]
#[cli::description("Get a configuration value")]
pub struct ConfigGet {
    name: String,
}

impl ConfigGet {
    #[tokio::main]
    pub async fn execute(&self) -> Result<(), Error> {
        let project
            = Project::new(None).await?;

        let camel_key
            = self.name.to_case(Case::Camel);
        let snake_key
            = self.name.to_case(Case::Snake);

        if let Ok(value) = project.config.project.get(&snake_key) {
            println!("{}", value.to_print_string());
        } else if let Ok(value) = project.config.user.get(&snake_key) {
            println!("{}", value.to_print_string());
        } else {
            return Err(Error::ConfigKeyNotFound(camel_key));
        }

        Ok(())
    }
}
