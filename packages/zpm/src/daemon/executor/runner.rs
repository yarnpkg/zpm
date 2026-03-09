use std::process::ExitStatus;
use std::sync::Arc;

use tokio::sync::mpsc;
use super::super::ipc::{TASK_CURRENT_ENV, DAEMON_SERVER_ENV};
use super::super::process_registry::ProcessRegistry;

use super::super::scheduler::PreparedTask;
use super::output::{stream_output, OutputLine};
use crate::error::Error;
use crate::script::ScriptEnvironment;

pub struct TaskRunner {
    prepared: PreparedTask,
    task_id: String,
    daemon_url: String,
    process_registry: Arc<ProcessRegistry>,
}

impl TaskRunner {
    pub fn new(prepared: PreparedTask, task_id: String, daemon_url: String, process_registry: Arc<ProcessRegistry>) -> Self {
        Self { prepared, task_id, daemon_url, process_registry }
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

        // Register the process PID for signal propagation
        let pid = running.child.id();
        if let Some(pid) = pid {
            self.process_registry.register(pid);
        }

        let child_stdout
            = running
                .child
                .stdout
                .take()
                .expect("Failed to capture stdout");

        let child_stderr
            = running
                .child
                .stderr
                .take()
                .expect("Failed to capture stderr");

        stream_output(child_stdout, child_stderr, output_tx).await;

        let status
            = running.child.wait().await?;

        // Unregister the process PID after it exits
        if let Some(pid) = pid {
            self.process_registry.unregister(pid);
        }

        Ok(status)
    }
}
