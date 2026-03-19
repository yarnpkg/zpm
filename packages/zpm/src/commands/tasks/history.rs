use std::{os::unix::process::ExitStatusExt, process::ExitStatus};

use clipanion::cli;
use zpm_utils::ToFileString;

use crate::daemon::{DaemonClient, TaskEvent};
use crate::error::Error;
use crate::project::Project;

/// Show the recent task event history
///
/// This command retrieves the most recent task events from the daemon,
/// including task starts, completions, failures, and cancellations.
/// Up to 1000 events are retained by the daemon.
#[cli::command]
#[cli::path("tasks", "history")]
#[cli::category("Task management commands")]
pub struct TaskHistory {
    /// Output as JSON (one event per line)
    #[cli::option("--json", default = false)]
    json: bool,
}

impl TaskHistory {
    pub async fn execute(&self) -> Result<ExitStatus, Error> {
        let project = Project::new(None).await?;

        let mut client = DaemonClient::connect(&project.project_cwd).await?;

        let events = client.get_task_history().await?;

        client.close();

        if self.json {
            for event in &events {
                println!("{}", serde_json::to_string(event).unwrap());
            }
        } else {
            self.print_human(&events);
        }

        Ok(ExitStatus::from_raw(0))
    }

    fn print_human(&self, events: &[TaskEvent]) {
        if events.is_empty() {
            println!("No task events recorded.");
            return;
        }

        for event in events {
            println!(
                "{} {} {}",
                event.date,
                event.contextual_task_id.to_file_string(),
                event.state,
            );
        }
    }
}
