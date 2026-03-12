// ============================================================================
// ExecutorPool - Command-Based
//
// Sends all events as commands to the coordinator instead of using a
// separate event channel. This eliminates the spawned event processing task.
// ============================================================================

use std::collections::HashSet;
use std::pin::Pin;
use std::process::ExitStatus;

use futures::stream::{FuturesUnordered, StreamExt};
use futures::Future;
use tokio::sync::mpsc;

use super::super::coordinator_commands::{CommandSender, CoordinatorCommand};
use super::super::coordinator_state::{format_contextual_task_id, ContextualTaskId, PreparedTask};
use super::output::OutputLine;
use super::runner::TaskRunner;
use crate::error::Error;

type TaskResult = (ContextualTaskId, Result<(ContextualTaskId, ExitStatus), Error>);
type TaskFuture = Pin<Box<dyn Future<Output = TaskResult> + Send>>;

/// ExecutorPool that communicates exclusively via commands.
/// No separate event channel - all notifications go through the coordinator.
pub struct ExecutorPool {
    tasks: FuturesUnordered<TaskFuture>,
    running: HashSet<ContextualTaskId>,
    daemon_url: String,
    command_tx: CommandSender,
}

impl ExecutorPool {
    pub fn new(daemon_url: String, command_tx: CommandSender) -> Self {
        Self {
            tasks: FuturesUnordered::new(),
            running: HashSet::new(),
            daemon_url,
            command_tx,
        }
    }

    pub fn spawn(&mut self, task_id: ContextualTaskId, prepared: PreparedTask) {
        let task_id_str = format_contextual_task_id(&task_id);
        let task_id_clone = task_id.clone();
        let task_id_for_result = task_id.clone();
        let daemon_url = self.daemon_url.clone();
        let command_tx = self.command_tx.clone();

        // Send TaskStarted command directly
        let _ = command_tx.send(CoordinatorCommand::TaskStarted {
            task_id: task_id_str.clone(),
        });

        // Create output channel that forwards to commands
        let (output_tx, mut output_rx) = mpsc::unbounded_channel::<OutputLine>();

        let command_tx_for_output = command_tx.clone();
        let task_id_for_output = task_id_str.clone();

        // Spawn output forwarder that sends commands instead of events
        tokio::spawn(async move {
            while let Some(output) = output_rx.recv().await {
                if command_tx_for_output
                    .send(CoordinatorCommand::TaskOutput {
                        task_id: task_id_for_output.clone(),
                        line: output.line,
                        stream: output.stream,
                    })
                    .is_err()
                {
                    // Command channel closed, stop processing output
                    break;
                }
            }
        });

        self.running.insert(task_id.clone());

        let future: TaskFuture = Box::pin(async move {
            let runner = TaskRunner::new(prepared, task_id_str, daemon_url, command_tx);
            let result = runner.run(output_tx).await;
            (task_id_for_result, result.map(|status| (task_id_clone, status)))
        });

        self.tasks.push(future);
    }

    pub fn running_tasks(&self) -> impl Iterator<Item = &ContextualTaskId> {
        self.running.iter()
    }

    pub fn is_empty(&self) -> bool {
        self.running.is_empty()
    }

    /// Wait for the next task to complete.
    /// Does NOT send completion events - the coordinator handles that
    /// based on the returned result.
    pub async fn wait_next(&mut self) -> Option<(ContextualTaskId, Result<ExitStatus, Error>)> {
        if self.running.is_empty() {
            return None;
        }

        let (completed_task_id, result) = self.tasks.next().await?;
        self.running.remove(&completed_task_id);

        match result {
            Ok((_, status)) => Some((completed_task_id, Ok(status))),
            Err(e) => Some((completed_task_id, Err(e))),
        }
    }
}
