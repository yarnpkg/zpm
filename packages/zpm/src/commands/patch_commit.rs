use clipanion::cli;
use zpm_primitives::{Ident, Locator, Reference};
use zpm_utils::{DataType, Path, FromFileString, ToFileString, ToHumanString};

use crate::{error::Error, git, project};

#[cli::command]
#[cli::path("patch-commit")]
#[cli::category("Dependency management")]
#[cli::description("Commit a patch for the package")]
pub struct PatchCommit {
    #[cli::option("-s,--save", default = false)]
    save: bool,

    source: Path,
}

impl PatchCommit {
    #[tokio::main()]
    pub async fn execute(&self) -> Result<(), Error> {
        let mut project
            = project::Project::new(None).await?;

        let locator_path = self.source
            .iter_path()
            .map(|path| path.with_join_str(".locator"))
            .find(|path| path.fs_exists())
            .ok_or_else(|| Error::NotAPatchFolder(self.source.clone()))?;

        let original_path = locator_path
            .dirname()
            .unwrap()
            .with_join_str("original");
        let user_path = self.source
            .dirname()
            .unwrap()
            .with_join_str("user");

        let locator_str = locator_path
            .fs_read_text()?;
        let locator
            = Locator::from_file_string(&locator_str)?;

        println!("{:?}", locator);
        println!("{:?}", original_path);
        println!("{:?}", user_path);

        let diff
            = git::diff_folders(&original_path, &user_path).await?;

        if !self.save {
            println!("{}", diff);
        }

        Ok(())
    }
}
