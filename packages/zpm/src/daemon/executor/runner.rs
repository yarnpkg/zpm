// ============================================================================
// TaskRunner - Command-Based
//
// Sends PID registration and unregistration via commands.
// Output is sent through the output channel (forwarded to commands by pool).
// ============================================================================

use std::process::ExitStatus;

use tokio::sync::mpsc;

use super::super::coordinator_commands::{CommandSender, CoordinatorCommand};
use super::super::coordinator_state::PreparedTask;
use super::super::ipc::{DAEMON_SERVER_ENV, TASK_CURRENT_ENV};
use super::output::{stream_output, OutputLine};
use crate::error::Error;
use crate::script::ScriptEnvironment;

pub struct TaskRunner {
    prepared: PreparedTask,
    task_id: String,
    daemon_url: String,
    command_tx: CommandSender,
}

impl TaskRunner {
    pub fn new(
        prepared: PreparedTask,
        task_id: String,
        daemon_url: String,
        command_tx: CommandSender,
    ) -> Self {
        Self {
            prepared,
            task_id,
            daemon_url,
            command_tx,
        }
    }

    pub async fn run(self, output_tx: mpsc::UnboundedSender<OutputLine>) -> Result<ExitStatus, Error> {
        let mut env = ScriptEnvironment::new()?;

        for (key, value) in &self.prepared.env {
            env = env.with_env_variable(key, value);
        }

        env = env.with_env_variable(TASK_CURRENT_ENV, &self.task_id);
        env = env.with_env_variable(DAEMON_SERVER_ENV, &self.daemon_url);

        let mut running = env
            .with_cwd(self.prepared.cwd.clone())
            .spawn_script(
                &self.prepared.script,
                self.prepared.args.iter().map(|s| s.as_str()),
            )
            .await?;

        // Register the process PID via command
        let pid = running.child.id();
        if let Some(pid) = pid {
            let _ = self.command_tx.send(CoordinatorCommand::RegisterPid {
                task_id: self.task_id.clone(),
                pid,
            });
        }

        let child_stdout = running
            .child
            .stdout
            .take()
            .ok_or_else(|| Error::TaskExecutionFailed("Failed to capture stdout".to_string()))?;

        let child_stderr = running
            .child
            .stderr
            .take()
            .ok_or_else(|| Error::TaskExecutionFailed("Failed to capture stderr".to_string()))?;

        stream_output(child_stdout, child_stderr, output_tx).await;

        let status = running.child.wait().await?;

        // Unregister the process PID via command
        if let Some(pid) = pid {
            let _ = self.command_tx.send(CoordinatorCommand::UnregisterPid {
                task_id: self.task_id.clone(),
                pid,
            });
        }

        Ok(status)
    }
}
