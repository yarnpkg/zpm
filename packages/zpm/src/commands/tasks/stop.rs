use std::{os::unix::process::ExitStatusExt, process::ExitStatus};

use clipanion::cli;
use zpm_utils::ToFileString;

use crate::daemon::DaemonClient;
use crate::error::Error;
use crate::project::Project;

#[cli::command]
#[cli::path("tasks", "stop")]
#[cli::category("Scripting commands")]
pub struct TaskStop {
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
            Ok(ExitStatus::from_raw(0))
        } else {
            let err_msg
                = error.unwrap_or_else(|| "Unknown error".to_string());

            eprintln!("Failed to stop task {}: {}", self.name, err_msg);
            Ok(ExitStatus::from_raw(1 << 8))
        }
    }
}
