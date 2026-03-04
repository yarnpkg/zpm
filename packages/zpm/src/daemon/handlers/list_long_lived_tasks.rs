use std::sync::Arc;
use std::time::UNIX_EPOCH;

use zpm_tasks::{parse, TaskId};
use zpm_utils::ToFileString;

use super::super::ipc::{DaemonResponse, LongLivedTaskInfo, LongLivedTaskStatus};
use super::super::long_lived::LongLivedRegistry;
use crate::project::Project;
use crate::tasks::TASK_FILE_NAME;

pub fn handle_list_long_lived_tasks(
    project: &Project,
    long_lived_registry: &Arc<LongLivedRegistry>,
) -> DaemonResponse {
    let mut tasks: Vec<LongLivedTaskInfo>
        = Vec::new();

    let running_entries
        = long_lived_registry.list_all_entries();

    for workspace in &project.workspaces {
        let task_file_path
            = workspace.path.with_join_str(TASK_FILE_NAME);

        let Ok(content) = task_file_path.fs_read_text() else {
            continue;
        };

        let Ok(task_file) = parse(&content) else {
            continue;
        };

        for (task_name, task) in &task_file.tasks {
            let is_long_lived
                = task.attributes.iter().any(|attr| attr.name == "long-lived");

            if !is_long_lived {
                continue;
            }

            let task_id
                = TaskId {
                    workspace: workspace.name.clone(),
                    task_name: task_name.clone(),
                };

            let status
                = running_entries
                    .iter()
                    .find(|entry| entry.task_id == task_id)
                    .map(|entry| {
                        let started_at_ms
                            = entry.started_at
                                .duration_since(UNIX_EPOCH)
                                .map(|d| d.as_millis() as u64)
                                .unwrap_or(0);

                        LongLivedTaskStatus::Running {
                            started_at_ms,
                            process_id: entry.process_id,
                        }
                    })
                    .unwrap_or(LongLivedTaskStatus::Stopped);

            tasks.push(LongLivedTaskInfo {
                workspace: workspace.name.to_file_string(),
                task_name: task_name.as_str().to_string(),
                status,
            });
        }
    }

    DaemonResponse::LongLivedTaskList { tasks }
}
