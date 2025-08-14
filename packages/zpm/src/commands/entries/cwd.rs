use std::{path::PathBuf, process::ExitCode};

use clipanion::{cli, prelude::Cli};

use crate::{commands::YarnCli, error::Error};

// TODO: Use clipanion to error on incorrect placement of `--cwd` argument.
#[cli::command(default, proxy)]
#[derive(Debug)]
pub struct Cwd {
    #[cli::option("--cwd")]
    cwd: String,

    args: Vec<String>,
}

impl Cwd {
    pub fn execute(&self) -> Result<ExitCode, Error> {
        std::env::set_current_dir(PathBuf::from(&self.cwd))?;

        Ok(YarnCli::run(self.cli_environment.clone().with_argv(self.args.clone())))
    }
}
