use clipanion::cli;

use crate::{error::Error, project::Project};

/// List the project's configuration values
#[cli::command]
#[cli::path("config")]
#[cli::path("config", "get")]
#[cli::category("Configuration commands")]
pub struct Config {
}

impl Config {
    pub async fn execute(&self) -> Result<(), Error> {
        let project
            = Project::new(None).await?;

        let tree
            = project.config.tree_node();

        print!("{}", tree.to_string());

        Ok(())
    }
}
