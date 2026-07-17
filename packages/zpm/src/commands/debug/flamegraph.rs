use std::process::{Command, ExitStatus};

use clipanion::cli;
use zpm_utils::Path;

use crate::{error::Error};

/// Profile a Yarn command with samply
///
/// This debug command runs the current Yarn binary under `samply record` and forwards all provided arguments to Yarn.
///
#[cli::command(proxy)]
#[cli::path("debug", "flamegraph")]
pub struct Flamegraph {
    /// Yarn command and arguments to profile
    args: Vec<String>,
}

impl Flamegraph {
    pub async fn execute(&self) -> Result<ExitStatus, Error> {
        let samply_path
            = Path::home_dir()?
                .ok_or(Error::HomeDirectoryNotFound)?
                .with_join_str(".cargo/bin/samply");

        if !samply_path.fs_exists() {
            return Err(Error::MissingSamply);
        }

        let current_exe
            = Path::current_exe()?;

        let result = Command::new(samply_path.to_path_buf())
            .args(["record", &current_exe.to_native_string()])
            .args(&self.args)
            .status()?;

        Ok(result)
    }
}
