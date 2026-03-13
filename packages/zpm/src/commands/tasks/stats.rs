use std::{os::unix::process::ExitStatusExt, process::ExitStatus};

use clipanion::cli;

use crate::daemon::DaemonClient;
use crate::error::Error;
use crate::project::Project;

/// Get internal state statistics from the daemon
///
/// This command returns statistics about the daemon's internal state,
/// useful for debugging and testing memory management.
#[cli::command]
#[cli::path("tasks", "stats")]
#[cli::category("Task management commands")]
pub struct TaskStats {
    /// Output as JSON
    #[cli::option("--json", default = false)]
    json: bool,
}

impl TaskStats {
    pub async fn execute(&self) -> Result<ExitStatus, Error> {
        let project = Project::new(None).await?;

        let mut client = DaemonClient::connect(&project.project_cwd).await?;

        let stats = client.get_stats().await?;

        client.close();

        if self.json {
            println!(
                "{}",
                serde_json::json!({
                    "tasksCount": stats.tasks_count,
                    "preparedCount": stats.prepared_count,
                    "subtasksCount": stats.subtasks_count,
                    "outputBufferCount": stats.output_buffer_count,
                    "closedTasksCount": stats.closed_tasks_count,
                })
            );
        } else {
            println!("Daemon State Statistics:");
            println!("  tasks:         {}", stats.tasks_count);
            println!("  prepared:      {}", stats.prepared_count);
            println!("  subtasks:      {}", stats.subtasks_count);
            println!("  output_buffer: {}", stats.output_buffer_count);
            println!("  closed_tasks:  {}", stats.closed_tasks_count);
        }

        Ok(ExitStatus::from_raw(0))
    }
}
