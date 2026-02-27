use std::os::unix::process::ExitStatusExt;
use std::process::ExitStatus;

use clipanion::cli;
use zpm_switch::{TaskSubscription, TASK_CURRENT_ENV};

use crate::daemon::DaemonClient;
use crate::error::Error;
use crate::project::Project;

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

        let project
            = Project::new(None).await?;

        let mut client
            = DaemonClient::connect(&project.project_cwd).await?;

        let task_subscriptions: Vec<TaskSubscription> = self
            .tasks
            .iter()
            .map(|name| TaskSubscription {
                name: name.clone(),
                subscriptions: vec![],
                args: vec![],
            })
            .collect();

        client.push_tasks(task_subscriptions, parent_task_id, None).await?;

        // On Unix, ExitStatus::from_raw expects the raw wait status where exit code is shifted by 8
        Ok(ExitStatus::from_raw(0 << 8))
    }
}
