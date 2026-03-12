mod cancel_context;
mod list_long_lived_tasks;
mod stop_task;

use std::sync::Arc;

use tokio::sync::{mpsc, oneshot};

use super::coordinator::CoordinatorCommand;
use super::ipc::{BufferedOutputLine, DaemonRequest, DaemonResponse};
use super::long_lived::LongLivedRegistry;
use super::process_registry::ProcessRegistry;
use super::scheduler::Scheduler;
use super::server::OutputBuffer;
use super::subscriptions::{SubscriptionId, SubscriptionRegistry};
use crate::project::Project;

pub use cancel_context::handle_cancel_context;
pub use list_long_lived_tasks::handle_list_long_lived_tasks;
pub use stop_task::handle_stop_task;

pub async fn dispatch_request(
    request: DaemonRequest,
    #[allow(unused_variables)]
    scheduler: &Scheduler,
    project: &Project,
    output_buffer: &OutputBuffer,
    subscription_registry: &SubscriptionRegistry,
    long_lived_registry: &Arc<LongLivedRegistry>,
    #[allow(unused_variables)]
    process_registry: &Arc<ProcessRegistry>,
    subscription_id: Option<SubscriptionId>,
    command_tx: &mpsc::UnboundedSender<CoordinatorCommand>,
) -> DaemonResponse {
    match request {
        DaemonRequest::Ping => DaemonResponse::Pong,
        DaemonRequest::PushTasks {
            tasks,
            parent_task_id,
            workspace,
            output_subscription: _,
            status_subscription: _,
            context_id,
        } => {
            let (response_tx, response_rx) = oneshot::channel();

            if command_tx.send(CoordinatorCommand::PushTasks {
                tasks,
                parent_task_id,
                workspace,
                context_id,
                response_tx,
            }).is_err() {
                return DaemonResponse::Error {
                    message: "Coordinator channel closed".to_string(),
                };
            }

            match response_rx.await {
                Ok(result) => {
                    if let Some(error) = result.error {
                        DaemonResponse::Error { message: error }
                    } else {
                        // Register subscriptions for the tasks
                        if let Some(sub_id) = subscription_id {
                            subscription_registry.add_tasks_to_subscription(
                                sub_id,
                                result.task_ids.clone(),
                                result.dependency_ids.clone(),
                            );
                        }

                        DaemonResponse::TasksEnqueued {
                            task_ids: result.task_ids,
                            dependency_count: result.dependency_ids.len(),
                            attached_long_lived: result.attached_long_lived,
                        }
                    }
                }
                Err(_) => DaemonResponse::Error {
                    message: "Coordinator did not respond".to_string(),
                },
            }
        }
        DaemonRequest::GetTaskOutput { task_id } => {
            let lines: Vec<BufferedOutputLine>
                = output_buffer
                    .read()
                    .ok()
                    .and_then(|buffer| buffer.get(&task_id).cloned())
                    .unwrap_or_default();

            DaemonResponse::TaskOutput { task_id, lines }
        }
        DaemonRequest::StopTask { task_name, workspace } => {
            handle_stop_task(
                &task_name,
                workspace.as_deref(),
                command_tx,
            ).await
        }
        DaemonRequest::ListLongLivedTasks => {
            handle_list_long_lived_tasks(project, long_lived_registry)
        }
        DaemonRequest::CancelContext { context_id } => {
            handle_cancel_context(
                &context_id,
                command_tx,
            ).await
        }
    }
}
