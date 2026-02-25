use std::os::unix::process::ExitStatusExt;
use std::process::ExitStatus;

use clipanion::cli;

use crate::error::Error;
use crate::ipc::{TaskIpcClient, IPC_CURRENT_TASK_ENV};

#[cli::command]
#[cli::path("task", "push")]
#[cli::category("Scripting commands")]
pub struct TaskPush {
    #[cli::positional]
    tasks: Vec<String>,
}

impl TaskPush {
    pub async fn execute(&self) -> Result<ExitStatus, Error> {
        if self.tasks.is_empty() {
            return Err(Error::TaskPushFailed("No tasks specified".to_string()));
        }

        let mut client
            = TaskIpcClient::connect().await?;

        let parent_task_id
            = std::env::var(IPC_CURRENT_TASK_ENV).ok();

        for task in &self.tasks {
            client.push_task(task, parent_task_id.as_deref()).await?;
        }

        Ok(ExitStatus::from_raw(0))
    }
}
