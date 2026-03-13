mod helpers;
mod runner;

pub mod list;
pub mod push;
pub mod run_buffered;
pub mod run_interlaced;
pub mod run_silent_dependencies;
pub mod stats;
pub mod stop;

use zpm_tasks::{parse, TaskName};

use crate::project::Project;

pub fn task_exists(project: &Project, task_name: &str) -> bool {
    let Ok(task_name) = TaskName::new(task_name) else {
        return false;
    };

    let Ok(workspace) = project.active_workspace() else {
        return false;
    };

    let task_file_path = workspace.taskfile_path();

    if !task_file_path.fs_exists() {
        return false;
    }

    let Ok(task_file_content) = task_file_path.fs_read_text() else {
        return false;
    };

    let Ok(task_file) = parse(&task_file_content) else {
        return false;
    };

    task_file.tasks.contains_key(task_name.as_str())
}
