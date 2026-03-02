mod push_tasks;

use super::ipc::{BufferedOutputLine, DaemonRequest, DaemonResponse};

use super::scheduler::Scheduler;
use super::server::OutputBuffer;
use super::subscriptions::{SubscriptionId, SubscriptionRegistry};
use crate::project::Project;

pub use push_tasks::handle_push_tasks;

pub fn dispatch_request(
    request: DaemonRequest,
    scheduler: &Scheduler,
    project: &Project,
    output_buffer: &OutputBuffer,
    subscription_registry: &SubscriptionRegistry,
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
            subscription_id,
        ),
        DaemonRequest::GetTaskOutput { task_id } => {
            let lines: Vec<BufferedOutputLine> = output_buffer
                .read()
                .ok()
                .and_then(|buffer| buffer.get(&task_id).cloned())
                .unwrap_or_default();
            DaemonResponse::TaskOutput { task_id, lines }
        }
    }
}
