use clipanion::cli;
use zpm_utils::ToFileString;

use crate::{error::Error, project};

/// List binaries available to the current workspace
///
/// This command lists executable binaries exposed by dependencies of the current workspace.
///
#[cli::command]
#[cli::path("bin")]
#[cli::category("Scripting commands")]
pub struct BinList {}

impl BinList {
    pub async fn execute(&self) -> Result<(), Error> {
        Ok(())
    }
}

/// Print the path of an accessible binary
///
/// This command prints the path to a binary exposed by one of the current workspace's dependencies. The reported path may point inside a zip archive.
///
#[cli::command]
#[cli::path("bin")]
#[cli::category("Scripting commands")]
pub struct Bin {
    /// Binary name to resolve
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

        let path = match binary {
            crate::script::Binary::Path {path, ..} => path,
            crate::script::Binary::PythonEntryPoint {..} => {
                return Err(Error::Unsupported);
            },
        };

        println!("{}", path.to_file_string());

        Ok(())
    }
}
