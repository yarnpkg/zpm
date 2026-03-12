use std::sync::Arc;

use zpm_primitives::Ident;
use zpm_tasks::{TaskId, TaskName};

use super::super::ipc::DaemonResponse;
use super::super::long_lived::LongLivedRegistry;
use super::super::process_registry::ProcessRegistry;
use crate::project::Project;

pub fn handle_stop_task(
    task_name: &str,
    workspace: Option<&str>,
    project: &Project,
    long_lived_registry: &Arc<LongLivedRegistry>,
    process_registry: &Arc<ProcessRegistry>,
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

    // Use the contextual_task_id to atomically claim the PID from the process registry.
    // This prevents a race condition where the task might complete naturally between
    // checking for its existence and attempting to kill it. By atomically removing
    // the PID from the registry, we ensure:
    // 1. If we get Some(pid), the PID is still valid and belongs to our task
    // 2. If we get None, the task already completed and we don't risk killing a reused PID
    let pid = process_registry.take_pid_for_task(&entry.contextual_task_id);

    if let Some(pid) = pid {
        #[cfg(unix)]
        {
            // Use killpg to kill the entire process group (since children are spawned with process_group(0))
            let result = unsafe { libc::killpg(pid as i32, libc::SIGTERM) };
            if result != 0 {
                // If killpg fails (e.g., group doesn't exist), try killing the process directly
                let result = unsafe { libc::kill(pid as i32, libc::SIGTERM) };
                if result != 0 && std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH) {
                    long_lived_registry.remove(&task_id);
                    return DaemonResponse::TaskStopped {
                        success: false,
                        error: Some(format!(
                            "Failed to send SIGTERM to process {}: {}",
                            pid,
                            std::io::Error::last_os_error()
                        )),
                    };
                }
            }
        }

        long_lived_registry.remove(&task_id);

        DaemonResponse::TaskStopped {
            success: true,
            error: None,
        }
    } else if entry.process_id.is_some() {
        // The entry had a PID recorded, but the process registry no longer has it.
        // This means the task completed naturally between when it was registered
        // and when we tried to stop it. Clean up the long-lived registry.
        long_lived_registry.remove(&task_id);

        DaemonResponse::TaskStopped {
            success: true,
            error: Some("Task already completed before stop request was processed".to_string()),
        }
    } else {
        // No PID was ever recorded for this task
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
