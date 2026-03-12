use std::process::ExitStatus;

use tokio::sync::mpsc;

use super::super::coordinator::CoordinatorCommand;
use super::super::ipc::{TASK_CURRENT_ENV, DAEMON_SERVER_ENV};
use super::super::scheduler::PreparedTask;
use super::output::{stream_output, OutputLine};
use crate::error::Error;
use crate::script::ScriptEnvironment;

pub struct TaskRunner {
    prepared: PreparedTask,
    task_id: String,
    daemon_url: String,
    command_tx: mpsc::UnboundedSender<CoordinatorCommand>,
}

impl TaskRunner {
    pub fn new(
        prepared: PreparedTask,
        task_id: String,
        daemon_url: String,
        command_tx: mpsc::UnboundedSender<CoordinatorCommand>,
    ) -> Self {
        Self { prepared, task_id, daemon_url, command_tx }
    }

    pub async fn run(
        self,
        output_tx: mpsc::UnboundedSender<OutputLine>,
    ) -> Result<ExitStatus, Error> {
        let mut env
            = ScriptEnvironment::new()?;

        for (key, value) in &self.prepared.env {
            env = env.with_env_variable(key, value);
        }

        env = env.with_env_variable(TASK_CURRENT_ENV, &self.task_id);
        env = env.with_env_variable(DAEMON_SERVER_ENV, &self.daemon_url);

        let mut running
            = env
                .with_cwd(self.prepared.cwd.clone())
                .spawn_script(
                    &self.prepared.script,
                    self.prepared.args.iter().map(|s| s.as_str()),
                )
                .await?;

        // Register the process PID via the coordinator command channel.
        // This ensures the coordinator can track spawning tasks and handle
        // race conditions with context cancellation.
        let pid = running.child.id();
        if let Some(pid) = pid {
            let _ = self.command_tx.send(CoordinatorCommand::RegisterPid {
                task_id: self.task_id.clone(),
                pid,
            });
        }

        let child_stdout
            = running
                .child
                .stdout
                .take()
                .ok_or_else(|| Error::TaskExecutionFailed("Failed to capture stdout".to_string()))?;

        let child_stderr
            = running
                .child
                .stderr
                .take()
                .ok_or_else(|| Error::TaskExecutionFailed("Failed to capture stderr".to_string()))?;

        stream_output(child_stdout, child_stderr, output_tx).await;

        let status
            = running.child.wait().await?;

        // Unregister the process PID after it exits
        if let Some(pid) = pid {
            let _ = self.command_tx.send(CoordinatorCommand::UnregisterPid {
                task_id: self.task_id.clone(),
                pid,
            });
        }

        Ok(status)
    }
}
