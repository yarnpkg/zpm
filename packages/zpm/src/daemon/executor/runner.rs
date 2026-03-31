use std::process::ExitStatus;

use zpm_utils::ToFileString;

use super::{
    super::{
        coordinator_commands::{CommandSender, CoordinatorCommand},
        coordinator_state::{ContextualTaskId, PreparedTask},
        ipc::{DAEMON_SERVER_ENV_NAME, CURRENT_TASK_ENV_NAME},
    },
    output::stream_output,
};
use crate::{
    error::Error,
    script::ScriptEnvironment,
};

pub struct TaskRunner {
    prepared: PreparedTask,
    task_id: ContextualTaskId,
    daemon_url: String,
    command_tx: CommandSender,
}

impl TaskRunner {
    pub fn new(
        prepared: PreparedTask,
        task_id: ContextualTaskId,
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

    pub async fn run(self) -> Result<ExitStatus, Error> {
        let mut env = ScriptEnvironment::new()?;

        let task_id_str = self.task_id.to_file_string();

        for (key, value) in &self.prepared.env {
            env = env.with_env_variable(key, value);
        }

        env = env.with_env_variable(CURRENT_TASK_ENV_NAME, &task_id_str);
        env = env.with_env_variable(DAEMON_SERVER_ENV_NAME, &self.daemon_url);

        let mut running = env
            .with_cwd(self.prepared.cwd.clone())
            .spawn_script(
                &self.prepared.script,
                self.prepared.args.iter().map(|s| s.as_str()),
            )
            .await?;

        // Get PID before sending TaskStarted so we can include it
        let pid = running.child.id();

        // Notify that the task has started (process is now spawned)
        let _ = self.command_tx.send(CoordinatorCommand::TaskStarted {
            task_id: self.task_id.clone(),
            pid,
        });
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

        stream_output(child_stdout, child_stderr, self.task_id.clone(), self.command_tx.clone()).await;

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
