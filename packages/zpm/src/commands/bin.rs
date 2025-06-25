use clipanion::cli;
use zpm_utils::ToFileString;

use crate::{error::Error, project};

#[cli::command]
#[cli::path("bin")]
#[derive(Debug)]
pub struct BinList {}

impl BinList {
    pub fn execute(&self) -> Result<(), Error> {
        Ok(())
    }
}

#[cli::command]
#[cli::path("bin")]
#[cli::category("Scripting commands")]
#[cli::description("Get the path of an accessible binary")]
pub struct Bin {
    name: String,
}

impl Bin {
    #[tokio::main()]
    pub async fn execute(&self) -> Result<(), Error> {
        let mut project
            = project::Project::new(None).await?;

        project
            .import_install_state()?;

        let binary
            = project.find_binary(&self.name)?;

        let binary_path = project.project_cwd
            .with_join(&binary.path);

        println!("{}", binary_path.to_file_string());

        Ok(())
    }
}
