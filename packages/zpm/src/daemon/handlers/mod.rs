mod push_tasks;
mod stop_task;

use std::sync::Arc;

use super::ipc::{BufferedOutputLine, DaemonRequest, DaemonResponse};
use super::long_lived::LongLivedRegistry;
use super::scheduler::Scheduler;
use super::server::OutputBuffer;
use super::subscriptions::{SubscriptionId, SubscriptionRegistry};
use crate::project::Project;

pub use push_tasks::handle_push_tasks;
pub use stop_task::handle_stop_task;

pub fn dispatch_request(
    request: DaemonRequest,
    scheduler: &Scheduler,
    project: &Project,
    output_buffer: &OutputBuffer,
    subscription_registry: &SubscriptionRegistry,
    long_lived_registry: &Arc<LongLivedRegistry>,
    subscription_id: Option<SubscriptionId>,
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
        } => handle_push_tasks(
            &tasks,
            parent_task_id.as_deref(),
            workspace.as_deref(),
            context_id.as_deref(),
            scheduler,
            project,
            subscription_registry,
            long_lived_registry,
            subscription_id,
        ),
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
                project,
                long_lived_registry,
            )
        }
    }
}
