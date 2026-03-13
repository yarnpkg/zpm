// ============================================================================
// ExecutorPool - Command-Based
//
// Sends ALL events as commands to the coordinator, including task completion.
// This ensures proper ordering: TaskOutput commands are always processed
// before TaskCompleted, since they all go through the same FIFO channel.
// ============================================================================

use std::collections::HashSet;
use std::pin::Pin;

use futures::stream::{FuturesUnordered, StreamExt};
use futures::Future;

use super::super::coordinator_commands::{CommandSender, CoordinatorCommand, TaskCompletionResult};
use super::super::coordinator_state::{format_contextual_task_id, ContextualTaskId, PreparedTask};
use super::runner::TaskRunner;

// The future just returns the task ID for bookkeeping; actual completion
// is sent through the command channel.
type TaskFuture = Pin<Box<dyn Future<Output = ContextualTaskId> + Send>>;

/// ExecutorPool that communicates exclusively via commands.
/// All events including completion go through the command channel.
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
        let task_id_for_result = task_id.clone();
        let daemon_url = self.daemon_url.clone();
        let command_tx = self.command_tx.clone();

        // Send TaskStarted command directly
        let _ = command_tx.send(CoordinatorCommand::TaskStarted {
            task_id: task_id_str.clone(),
        });

        self.running.insert(task_id.clone());

        let future: TaskFuture = Box::pin(async move {
            let runner = TaskRunner::new(prepared, task_id_str.clone(), daemon_url, command_tx.clone());
            let result = runner.run().await;

            // Send TaskCompleted through the command channel.
            // This happens AFTER stream_output() completes, so all TaskOutput
            // commands are already in the channel ahead of this one.
            let completion_result = match result {
                Ok(status) => TaskCompletionResult::Exited(status),
                Err(e) => TaskCompletionResult::Error(e.to_string()),
            };

            let _ = command_tx.send(CoordinatorCommand::TaskCompleted {
                task_id: task_id_str,
                result: completion_result,
            });

            task_id_for_result
        });

        self.tasks.push(future);
    }

    pub fn running_tasks(&self) -> impl Iterator<Item = &ContextualTaskId> {
        self.running.iter()
    }

    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }

    /// Mark a task as no longer running.
    /// Called when TaskCompleted is processed by the coordinator.
    pub fn mark_completed(&mut self, task_id: &ContextualTaskId) {
        self.running.remove(task_id);
    }

    /// Poll the next task completion to drive futures forward.
    /// Does NOT update the running set - that's done when TaskCompleted is processed.
    pub async fn poll_next(&mut self) {
        if self.tasks.is_empty() {
            return;
        }

        // Just drive the futures forward, don't update running set
        let _ = self.tasks.next().await;
    }
}
