use clipanion::cli;

use crate::{error::{self, Error}, project};

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
pub struct Bin {
    name: String,
}

impl Bin {
    pub fn execute(&self) -> Result<(), Error> {
        let mut project
            = project::Project::new(None)?;

        project
            .import_install_state()?;

        let binary
            = project.find_binary(&self.name)?;

        let binary_path = project.project_cwd
            .with_join(&binary.path);

        println!("{}", binary_path.to_string());

        Ok(())
    }
}
