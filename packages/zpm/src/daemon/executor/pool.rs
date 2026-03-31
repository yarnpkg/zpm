use std::collections::HashSet;

use super::{
    super::{
        coordinator_commands::{CommandSender, CoordinatorCommand, TaskCompletionResult},
        coordinator_state::{ContextualTaskId, PreparedTask},
    },
    runner::TaskRunner,
};

/// ExecutorPool that communicates exclusively via commands.
/// All events including completion go through the command channel.
pub struct ExecutorPool {
    running: HashSet<ContextualTaskId>,
    daemon_url: String,
    command_tx: CommandSender,
}

impl ExecutorPool {
    pub fn new(daemon_url: String, command_tx: CommandSender) -> Self {
        Self {
            running: HashSet::new(),
            daemon_url,
            command_tx,
        }
    }

    pub fn spawn(&mut self, task_id: ContextualTaskId, prepared: PreparedTask) {
        let daemon_url = self.daemon_url.clone();
        let command_tx = self.command_tx.clone();

        self.running.insert(task_id.clone());

        tokio::spawn(async move {
            let runner = TaskRunner::new(prepared, task_id.clone(), daemon_url, command_tx.clone());
            let result = runner.run().await;

            // Send TaskCompleted through the command channel.
            // This happens AFTER stream_output() completes, so all TaskOutput
            // commands are already in the channel ahead of this one.
            let completion_result = match result {
                Ok(status) => TaskCompletionResult::Exited(status),
                Err(e) => TaskCompletionResult::Error(e.to_string()),
            };

            let _ = command_tx.send(CoordinatorCommand::TaskCompleted {
                task_id,
                result: completion_result,
            });
        });
    }

    pub fn running_tasks(&self) -> impl Iterator<Item = &ContextualTaskId> {
        self.running.iter()
    }

    /// Mark a task as no longer running.
    /// Called when TaskCompleted is processed by the coordinator.
    pub fn mark_completed(&mut self, task_id: &ContextualTaskId) {
        self.running.remove(task_id);
    }
}
