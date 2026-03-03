use std::sync::Arc;

use zpm_primitives::Ident;
use zpm_tasks::{TaskId, TaskName};

use super::super::ipc::DaemonResponse;
use super::super::long_lived::LongLivedRegistry;
use crate::project::Project;

pub fn handle_stop_task(
    task_name: &str,
    workspace: Option<&str>,
    project: &Project,
    long_lived_registry: &Arc<LongLivedRegistry>,
) -> DaemonResponse {
    let task_id
        = match build_task_id(task_name, workspace, project) {
            Some(tid) => tid,
            None => {
                return DaemonResponse::TaskStopped {
                    success: false,
                    error: Some(format!("Could not resolve task: {}", task_name)),
                };
            }
        };

    let entry
        = match long_lived_registry.get_existing(&task_id) {
            Some(e) => e,
            None => {
                return DaemonResponse::TaskStopped {
                    success: false,
                    error: Some(format!("No running long-lived task found: {}", task_name)),
                };
            }
        };

    if let Some(pid) = entry.process_id {
        let _ = std::process::Command::new("kill")
            .arg("-TERM")
            .arg(pid.to_string())
            .status();

        long_lived_registry.remove(&task_id);

        DaemonResponse::TaskStopped {
            success: true,
            error: None,
        }
    } else {
        long_lived_registry.remove(&task_id);

        DaemonResponse::TaskStopped {
            success: true,
            error: Some("Task had no process ID, removed from registry".to_string()),
        }
    }
}

fn build_task_id(task_name: &str, workspace: Option<&str>, project: &Project) -> Option<TaskId> {
    let task_name
        = TaskName::new(task_name).ok()?;

    let workspace
        = if let Some(ws_name) = workspace {
            let ident
                = Ident::new(ws_name);

            project.workspace_by_ident(&ident).ok()?.name.clone()
        } else {
            project.active_workspace().ok()?.name.clone()
        };

    Some(TaskId { workspace, task_name })
}
