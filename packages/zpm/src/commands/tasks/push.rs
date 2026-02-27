use std::os::unix::process::ExitStatusExt;
use std::process::ExitStatus;

use clipanion::cli;
use zpm_switch::{TaskSubscription, TASK_CURRENT_ENV};

use crate::daemon::DaemonClient;
use crate::error::Error;

#[cli::command]
#[cli::path("tasks", "push")]
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

        // tasks push can only be called from within a running task
        let parent_task_id = match std::env::var(TASK_CURRENT_ENV) {
            Ok(id) => Some(id),
            Err(_) => {
                return Err(Error::TaskPushFailed(
                    format!("Not running inside a task context ({} not set)", TASK_CURRENT_ENV),
                ));
            }
        };

        let mut client = DaemonClient::connect().await?;

        let task_subscriptions: Vec<TaskSubscription> = self
            .tasks
            .iter()
            .map(|name| TaskSubscription {
                name: name.clone(),
                subscriptions: vec![],
            })
            .collect();

        client.push_tasks(task_subscriptions, parent_task_id).await?;

        Ok(ExitStatus::from_raw(0))
    }
}
