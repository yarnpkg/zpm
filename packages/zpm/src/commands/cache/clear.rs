use clipanion::cli;
use zpm_utils::ToFileString;

use crate::{
    error::Error,
    project::Project,
};

/// Remove the shared cache files
///
/// This command will remove all the files from the cache. This is an alias for `yarn cache clean`.
///
#[cli::command]
#[cli::path("cache", "clear")]
#[cli::category("Cache commands")]
pub struct Clear {}

impl Clear {
    pub async fn execute(&self) -> Result<(), Error> {
        let project
            = Project::new(None).await?;

        let cache_path = project.preferred_cache_path();

        if cache_path.fs_exists() {
            cache_path.fs_rm()?;
            println!("Cache directory cleared: {}", cache_path.to_file_string());
        } else {
            println!("Cache directory does not exist: {}", cache_path.to_file_string());
        }

        Ok(())
    }
}
