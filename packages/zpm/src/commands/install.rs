use clipanion::cli;

use crate::{error::Error, print_time, project};

#[cli::command(default)]
#[cli::path("install")]
pub struct Install {
}

impl Install {
    #[tokio::main()]
    pub async fn execute(&self) -> Result<(), Error> {
        print_time!("Before project");
        let mut project
            = project::Project::new(None)?;

        print_time!("Before install");
        project.run_install().await?;

        Ok(())
    }
}

