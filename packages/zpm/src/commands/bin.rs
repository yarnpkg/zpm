use clipanion::cli;
use zpm_utils::ToFileString;

use crate::{error::Error, project};

/// Print the list of all the binaries available in the current workspace.
///
/// Adding the \`-v,--verbose\` flag will cause the output to contain both the binary name and the locator of the package that provides the binary.
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
/// Print the path to the binary on the standard output and exit. Note that the reported path may be stored within a zip archive.
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
