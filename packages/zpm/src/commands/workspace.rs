use std::{path::PathBuf, process::ExitCode};

use zpm_primitives::Ident;
use clipanion::{cli, prelude::*};

use crate::{
    error::Error,
    project::Project,
};

use super::YarnCli;

/// Run a command in a workspace
#[cli::command(proxy)]
#[cli::path("workspace")]
#[cli::category("Workspace commands")]
pub struct Workspace {
    workspace: Ident,
    args: Vec<String>,
}

impl Workspace {
    pub async fn execute(&self) -> Result<ExitCode, Error> {
        let project
            = Project::new(None).await?;

        let workspace
            = project.workspace_by_ident(&self.workspace)?;

        let cwd
            = workspace.path.clone();

        std::env::set_current_dir(PathBuf::from(cwd.as_str()))?;

        let env = self.cli_environment.clone().with_argv(self.args.clone());
        Ok(tokio::task::block_in_place(move || {
            tokio::runtime::Handle::current().block_on(async move {
                YarnCli::run(env).await
            })
        }))
    }
}
