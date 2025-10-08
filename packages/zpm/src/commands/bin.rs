use clipanion::cli;
use zpm_utils::ToFileString;

use crate::{error::Error, project};

#[cli::command]
#[cli::path("bin")]
#[derive(Debug)]
pub struct BinList {}

impl BinList {
    pub async fn execute(&self) -> Result<(), Error> {
        Ok(())
    }
}

/// Get the path of an accessible binary
#[cli::command]
#[cli::path("bin")]
#[cli::category("Scripting commands")]
pub struct Bin {
    name: String,
}

impl Bin {
    pub async fn execute(&self) -> Result<(), Error> {
        let mut project
            = project::Project::new(None).await?;

        project
            .lazy_install().await?;

        let binary
            = project.find_binary(&self.name)?;

        let binary_path = project.project_cwd
            .with_join(&binary.path);

        println!("{}", binary_path.to_file_string());

        Ok(())
    }
}
