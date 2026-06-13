use std::process::ExitStatus;

use clipanion::cli;
use colored::Colorize;

use crate::daemon::{DaemonClient, LongLivedTaskInfo, LongLivedTaskStatus};
use crate::error::Error;
use crate::project::Project;

use super::helpers::format_start_time;

/// List all long-lived tasks
///
/// This command lists all long-lived tasks currently registered in the project.
/// Long-lived tasks are background processes that continue running after the
/// initial command completes, such as development servers or watch processes.
///
/// The output shows each task's name, current status (running or stopped),
/// and when it was started (for running tasks).
#[cli::command]
#[cli::path("tasks")]
#[cli::category("Task management commands")]
pub struct TaskList {
    /// Format the output as an NDJSON stream
    #[cli::option("--json", default = false)]
    pub json: bool,
}

impl TaskList {
    pub async fn execute(&self) -> Result<ExitStatus, Error> {
        let project
            = Project::new(None).await?;

        let mut client
            = DaemonClient::connect(&project.project_cwd).await?;

        let tasks
            = client.list_long_lived_tasks().await?;

        client.close();

        if self.json {
            self.print_json(&tasks);
        } else {
            self.print_human(&tasks);
        }

        Ok(super::runner::exit_status_from_code(0))
    }

    fn print_json(&self, tasks: &[LongLivedTaskInfo]) {
        for task in tasks {
            println!("{}", serde_json::to_string(task).unwrap());
        }
    }

    fn print_human(&self, tasks: &[LongLivedTaskInfo]) {
        if tasks.is_empty() {
            println!("No long-lived tasks found in this project.");
            return;
        }

        println!("{}", "Long-lived tasks:".bold());
        println!();

        for task in tasks {
            let task_display
                = format!("{}:{}", task.workspace, task.task_name);

            match &task.status {
                LongLivedTaskStatus::Stopped => {
                    println!(
                        "  {} {}",
                        task_display.bold(),
                        "(stopped)".dimmed()
                    );
                }
                LongLivedTaskStatus::Running { started_at_ms, process_id } => {
                    let started_str
                        = format_start_time(*started_at_ms);

                    let pid_str
                        = process_id
                            .map(|pid| format!(" (pid: {})", pid))
                            .unwrap_or_default();

                    println!(
                        "  {} {} {}{}",
                        task_display.bold(),
                        "running".green(),
                        format!("since {}", started_str).dimmed(),
                        pid_str.dimmed()
                    );
                }
            }
        }

        println!();
    }
}
