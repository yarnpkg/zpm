use clipanion::cli;
use convert_case::{Case, Casing};
use zpm_utils::ToHumanString;

use crate::{error::Error, project::Project};

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

        let project_settings
            = project.config.project.to_btree_map();

        let camel_key
            = self.name.to_case(Case::Camel);
        let snake_key
            = self.name.to_case(Case::Snake);

        let value
            = project_settings.get(&snake_key)
                .ok_or_else(|| Error::ConfigKeyNotFound(camel_key))?;

        println!("{}", value.to_print_string());

        Ok(())
    }
}
