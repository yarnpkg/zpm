use std::sync::Arc;

use zpm_primitives::Ident;
use zpm_tasks::{TaskId, TaskName};

use std::time::SystemTime;

use super::super::ipc::{AttachedLongLivedTask, DaemonResponse, TaskSubscription, LONG_LIVED_CONTEXT_ID};
use super::super::long_lived::LongLivedRegistry;
use super::super::scheduler::{format_contextual_task_id, Scheduler};
use super::super::subscriptions::{SubscriptionId, SubscriptionRegistry};
use crate::project::Project;

pub fn handle_push_tasks(
    tasks: &[TaskSubscription],
    parent_task_id: Option<&str>,
    workspace: Option<&str>,
    context_id: Option<&str>,
    scheduler: &Scheduler,
    project: &Project,
    subscription_registry: &SubscriptionRegistry,
    long_lived_registry: &Arc<LongLivedRegistry>,
    subscription_id: Option<SubscriptionId>,
) -> DaemonResponse {
    let mut task_ids
        = Vec::new();

    let mut dependency_ids
        = Vec::new();

    let mut total_dependency_count
        = 0;

    let mut attached_long_lived
        = Vec::new();

    for task_sub in tasks {
        let task_id
            = build_task_id(&task_sub.name, workspace, project);

        let is_long_lived
            = task_id
                .as_ref()
                .and_then(|tid| check_if_long_lived(project, tid))
                .unwrap_or(false);

        if is_long_lived {
            if let Some(ref tid) = task_id {
                if let Some(existing) = long_lived_registry.get_existing(tid) {
                    task_ids.push(existing.contextual_task_id.clone());

                    let started_at_ms
                        = existing
                            .started_at
                            .duration_since(SystemTime::UNIX_EPOCH)
                            .map(|d| d.as_millis() as u64)
                            .unwrap_or(0);

                    attached_long_lived.push(AttachedLongLivedTask {
                        task_id: existing.contextual_task_id.clone(),
                        started_at_ms,
                    });

                    if let Some(sub_id) = subscription_id {
                        subscription_registry.add_tasks_to_subscription(
                            sub_id,
                            vec![existing.contextual_task_id],
                            vec![],
                        );
                    }

                    continue;
                }
            }
        }

        let effective_context_id
            = if is_long_lived {
                Some(LONG_LIVED_CONTEXT_ID)
            } else {
                context_id
            };

        match scheduler.add_task(
            project,
            &task_sub.name,
            parent_task_id,
            task_sub.args.clone(),
            workspace,
            effective_context_id,
        ) {
            Ok((ctx_task_id, resolved_ctx_task_ids)) => {
                let target_id_str
                    = format_contextual_task_id(&ctx_task_id);

                if is_long_lived {
                    if let Some(tid) = task_id {
                        long_lived_registry.register(tid, target_id_str.clone());
                    }
                }

                task_ids.push(target_id_str.clone());

                for resolved_id in &resolved_ctx_task_ids {
                    let resolved_str
                        = format_contextual_task_id(resolved_id);

                    if resolved_str != target_id_str {
                        dependency_ids.push(resolved_str);
                    }
                }

                total_dependency_count += resolved_ctx_task_ids.len().saturating_sub(1);
            }
            Err(e) => {
                return DaemonResponse::Error {
                    message: e.to_string(),
                };
            }
        }
    }

    if let Some(sub_id) = subscription_id {
        subscription_registry.add_tasks_to_subscription(
            sub_id,
            task_ids.clone(),
            dependency_ids,
        );
    }

    DaemonResponse::TasksEnqueued {
        task_ids,
        dependency_count: total_dependency_count,
        attached_long_lived,
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

fn check_if_long_lived(project: &Project, task_id: &TaskId) -> Option<bool> {
    let workspace
        = project.workspace_by_ident(&task_id.workspace).ok()?;

    let task_file_path
        = workspace.taskfile_path();

    let content
        = task_file_path.fs_read_text().ok()?;

    let task_file
        = zpm_tasks::parse(&content).ok()?;

    let task
        = task_file.tasks.get(task_id.task_name.as_str())?;

    Some(task.attributes.iter().any(|attr| attr.name == "long-lived"))
}
