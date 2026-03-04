use std::os::unix::process::ExitStatusExt;
use std::process::ExitStatus;

use clipanion::cli;
use crate::daemon::{DaemonClient, TaskSubscription, DAEMON_SERVER_ENV, TASK_CURRENT_ENV};
use crate::error::Error;

/// Push tasks to be executed from within a running task
///
/// This command allows a running task to schedule additional tasks to be
/// executed by the daemon. It can only be called from within a task context
/// (i.e., when running inside a task that was started by the daemon).
///
/// This is useful for dynamically spawning subtasks based on runtime conditions,
/// such as triggering build steps after certain conditions are met.
#[cli::command]
#[cli::path("tasks", "push")]
#[cli::category("Task management commands")]
pub struct TaskPush {
    /// Names of the tasks to push for execution
    #[cli::positional]
    tasks: Vec<String>,
}

impl TaskPush {
    pub async fn execute(&self) -> Result<ExitStatus, Error> {
        if self.tasks.is_empty() {
            return Err(Error::TaskPushFailed("No tasks specified".to_string()));
        }

        let parent_task_id
            = match std::env::var(TASK_CURRENT_ENV) {
                Ok(id) => Some(id),
                Err(_) => {
                    return Err(Error::TaskPushFailed(
                        format!("Not running inside a task context ({} not set)", TASK_CURRENT_ENV),
                    ));
                }
            };

        let daemon_url
            = match std::env::var(DAEMON_SERVER_ENV) {
                Ok(url) => url,
                Err(_) => {
                    return Err(Error::TaskPushFailed(
                        format!("Not running inside a daemon context ({} not set)", DAEMON_SERVER_ENV),
                    ));
                }
            };

        let mut client
            = DaemonClient::connect_to_url(&daemon_url).await?;

        let task_subscriptions: Vec<TaskSubscription>
            = self
                .tasks
                .iter()
                .map(|name| TaskSubscription {
                    name: name.clone(),
                    args: vec![],
                })
                .collect();

        client.push_tasks(task_subscriptions, parent_task_id, None, None).await?;

        Ok(ExitStatus::from_raw(0 << 8))
    }
}
