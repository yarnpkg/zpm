use clipanion::cli;
use zpm_tasks::{TaskId, TaskName};

use crate::{error::Error, project};

#[cli::command]
#[cli::path("debug", "resolve-task")]
pub struct ResolveTask {
    name: String,
}

impl ResolveTask {
    pub async fn execute(&self) -> Result<(), Error> {
        let task_name
            = TaskName::new(&self.name)
                .map_err(|e| Error::TaskNameParseError(e.to_string()))?;

        let project
            = project::Project::new(None).await?;

        let workspace
            = project.active_workspace()?;

        let root_task
            = TaskId {
                workspace: workspace.name.clone(),
                task_name,
            };

        let resolved
            = project.resolve_task(&root_task)?;

        let json
            = serde_json::to_string_pretty(&resolved.tasks)
                .map_err(|e| Error::JsonSerializeError(e.to_string()))?;

        println!("{}", json);

        Ok(())
    }
}
