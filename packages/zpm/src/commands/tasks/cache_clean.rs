use std::process::ExitStatus;

use clipanion::cli;
use zpm_utils::{
    IoResultExt,
    ToHumanString,
};

use crate::{
    error::Error,
    project::Project,
    task_cache::task_cache_state_path,
};

/// Remove all the entries from the task cache
///
/// This command removes the results stored by the tasks declared with the `@cache` attribute, along with the file hashes Yarn memoizes to
/// speed up the cache lookups. The next run of each cached task will execute its script.
///
#[cli::command]
#[cli::path("tasks", "cache", "clean")]
#[cli::category("Task management commands")]
pub struct TaskCacheClean {
}

impl TaskCacheClean {
    pub async fn execute(&self) -> Result<ExitStatus, Error> {
        let project
            = Project::new(None).await?;

        let cache_folder
            = project.project_path(&project.config.settings.task_cache_folder.value);

        cache_folder
            .fs_rm()
            .ok_missing()?;

        task_cache_state_path(&project)
            .fs_rm()
            .ok_missing()?;

        println!("Removed {}", cache_folder.to_print_string());

        Ok(super::runner::exit_status_from_code(0))
    }
}
