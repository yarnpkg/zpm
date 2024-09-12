use clipanion::cli;

use crate::{error::Error, project};

#[cli::command(default)]
#[cli::path("install")]
pub struct Install {
}

impl Install {
    #[tokio::main()]
    pub async fn execute(&self) -> Result<(), Error> {
        let mut project
            = project::Project::new(None)?;

        project.run_install().await?;

        Ok(())
    }
}

