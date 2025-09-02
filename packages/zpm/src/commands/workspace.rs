use std::{path::PathBuf, process::ExitCode};

use zpm_primitives::Ident;
use zpm_utils::Path;
use clipanion::{cli, prelude::Cli};

use crate::{
    error::Error,
    project::Project,
};

use super::YarnCli;

#[cli::command(proxy)]
#[cli::path("workspace")]
#[cli::category("Workspace commands")]
#[cli::description("Run a command in a workspace")]
pub struct Workspace {
    workspace: Ident,
    args: Vec<String>,
}

impl Workspace {
    pub fn execute(&self) -> Result<ExitCode, Error> {
        let cwd
            = self.get_cwd()?;

        std::env::set_current_dir(PathBuf::from(cwd.as_str()))?;

        Ok(YarnCli::run(self.cli_environment.clone().with_argv(self.args.clone())))
    }

    #[tokio::main()]
    async fn get_cwd(&self) -> Result<Path, Error> {
        let project
            = Project::new(None).await?;

        let workspace
            = project.workspace_by_ident(&self.workspace)?;

        Ok(workspace.path.clone())
    }
}
