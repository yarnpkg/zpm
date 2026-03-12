use tokio::sync::{mpsc, oneshot};

use super::super::coordinator::CoordinatorCommand;
use super::super::ipc::DaemonResponse;

/// Handle a context cancellation request by sending a command to the coordinator.
/// This ensures all state mutations happen in a single async task, eliminating
/// race conditions with spawning tasks.
pub async fn handle_cancel_context(
    context_id: &str,
    command_tx: &mpsc::UnboundedSender<CoordinatorCommand>,
) -> DaemonResponse {
    let (response_tx, response_rx) = oneshot::channel();

    // Send the cancel command to the coordinator
    if command_tx.send(CoordinatorCommand::CancelContext {
        context_id: context_id.to_string(),
        response_tx,
    }).is_err() {
        return DaemonResponse::Error {
            message: "Coordinator channel closed".to_string(),
        };
    }

    // Wait for the coordinator to process the cancellation
    match response_rx.await {
        Ok(result) => DaemonResponse::ContextCancelled {
            cancelled_count: result.cancelled_count,
        },
        Err(_) => DaemonResponse::Error {
            message: "Coordinator did not respond".to_string(),
        },
    }
}
