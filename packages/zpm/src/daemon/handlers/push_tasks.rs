use super::super::ipc::{DaemonResponse, TaskSubscription};

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
    subscription_id: Option<SubscriptionId>,
) -> DaemonResponse {
    let mut task_ids = Vec::new();
    let mut dependency_ids = Vec::new();
    let mut total_dependency_count = 0;

    for task_sub in tasks {
        match scheduler.add_task(
            project,
            &task_sub.name,
            parent_task_id,
            task_sub.args.clone(),
            workspace,
            context_id,
        ) {
            Ok((ctx_task_id, resolved_ctx_task_ids)) => {
                let target_id_str = format_contextual_task_id(&ctx_task_id);
                task_ids.push(target_id_str.clone());

                // Collect dependency IDs (excluding target)
                for resolved_id in &resolved_ctx_task_ids {
                    let resolved_str = format_contextual_task_id(resolved_id);
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

    // Atomically register task IDs with subscription BEFORE returning
    // This ensures the subscription filter is ready before any notifications
    // can be generated for these tasks
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
    }
}
