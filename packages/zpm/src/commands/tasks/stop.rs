use std::process::ExitStatus;

use clipanion::cli;
use zpm_utils::ToFileString;

use crate::daemon::DaemonClient;
use crate::error::Error;
use crate::project::Project;

/// Stop a running long-lived task
///
/// This command stops a long-lived task that is currently running in the
/// background. The task will be terminated and its status will be set to
/// "stopped".
///
/// Use `yarn tasks list` to see the names of running tasks that can be stopped.
#[cli::command]
#[cli::path("tasks", "stop")]
#[cli::category("Task management commands")]
pub struct TaskStop {
    /// Name of the task to stop
    name: String,
}

impl TaskStop {
    pub async fn execute(&self) -> Result<ExitStatus, Error> {
        let project
            = Project::new(None).await?;

        let workspace
            = project.active_workspace()?;

        let workspace_name
            = workspace.name.to_file_string();

        let mut client
            = DaemonClient::connect(&project.project_cwd).await?;

        let (success, error)
            = client.stop_task(&self.name, Some(workspace_name)).await?;

        client.close();

        if success {
            println!("Task {} stopped successfully", self.name);
            Ok(super::runner::exit_status_from_code(0))
        } else {
            let err_msg
                = error.unwrap_or_else(|| "Unknown error".to_string());

            eprintln!("Failed to stop task {}: {}", self.name, err_msg);
            Ok(super::runner::exit_status_from_code(1))
        }
    }
}
