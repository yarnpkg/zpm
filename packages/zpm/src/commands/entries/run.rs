use std::{process::ExitCode, str::FromStr};

use clipanion::{cli, prelude::Cli};
use zpm_utils::{ExplicitPath, PathError};

use crate::{commands::YarnCli, error::Error};

#[cli::command(default, proxy)]
#[derive(Debug)]
pub struct Run {
    leading_argument: String,

    args: Vec<String>,
}

impl Run {
    pub fn execute(&self) -> Result<ExitCode, Error> {
        match ExplicitPath::from_str(&self.leading_argument) {
            Ok(explicit_path) => {
                std::env::set_current_dir(explicit_path.raw_path.path.to_path_buf())?;

                Ok(YarnCli::run(self.cli_environment.clone().with_argv(self.args.clone())))
            },

            Err(PathError::InvalidExplicitPathParameter(_)) => {
                Ok(YarnCli::run(self.cli_environment.clone().with_argv(
                    ["run".to_owned(), self.leading_argument.clone()]
                        .into_iter()
                        .chain(self.args.clone())
                        .collect()
                )))
            },

            Err(err) => Err(err.into()),
        }
    }
}
