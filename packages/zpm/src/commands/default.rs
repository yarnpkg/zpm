use std::{process::ExitCode, str::FromStr};

use clipanion::{advanced::Cli, cli};

use crate::{error::Error, primitives::Ident};

use super::YarnCli;

#[cli::command(default, proxy)]
pub struct Default {
    leading_argument: String,
    args: Vec<String>,
}

impl Default {
    pub fn execute(&self) -> Result<ExitCode, Error> {
        let dir_separator = self.leading_argument.chars()
            .find(|c| *c == '\\' || *c == '/');

        if dir_separator.is_some() && !Ident::from_str(&self.leading_argument).is_ok() {
            std::env::set_current_dir(std::path::PathBuf::from(&self.leading_argument))?;

            Ok(YarnCli::run(self.cli_info.with_argv(self.args.clone())))
        } else {
            let mut argv
                = vec!["run".to_string(), self.leading_argument.clone()];

            argv.extend(self.args.iter().cloned());

            Ok(YarnCli::run(self.cli_info.with_argv(argv)))
        }
    }
}
