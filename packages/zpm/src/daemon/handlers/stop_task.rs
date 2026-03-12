use tokio::sync::{mpsc, oneshot};

use super::super::coordinator::CoordinatorCommand;
use super::super::ipc::DaemonResponse;

/// Handle a stop task request by sending a command to the coordinator.
/// This ensures all state mutations happen in a single async task, eliminating
/// race conditions with spawning tasks.
pub async fn handle_stop_task(
    task_name: &str,
    workspace: Option<&str>,
    command_tx: &mpsc::UnboundedSender<CoordinatorCommand>,
) -> DaemonResponse {
    let (response_tx, response_rx) = oneshot::channel();

    // Send the stop command to the coordinator
    if command_tx.send(CoordinatorCommand::StopTask {
        task_name: task_name.to_string(),
        workspace: workspace.map(String::from),
        response_tx,
    }).is_err() {
        return DaemonResponse::Error {
            message: "Coordinator channel closed".to_string(),
        };
    }

    // Wait for the coordinator to process the stop request
    match response_rx.await {
        Ok(result) => DaemonResponse::TaskStopped {
            success: result.success,
            error: result.error,
        },
        Err(_) => DaemonResponse::Error {
            message: "Coordinator did not respond".to_string(),
        },
    }
}
