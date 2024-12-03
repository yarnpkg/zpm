use clipanion::cli;

use crate::{error::Error, print_time, project};

#[cli::command(default)]
#[cli::path("install")]
pub struct Install {
    #[cli::option("--exit")]
    exit: bool,
}

impl Install {
    #[tokio::main()]
    pub async fn execute(&self) -> Result<(), Error> {
        print_time!("Before project");
        let mut project
            = project::Project::new(None)?;

        project
            .import_install_state();

        print_time!("Before install");
        project.run_install().await?;

        if self.exit {
            panic!("Exiting as requested");
        }

        Ok(())
    }
}

