use std::process::ExitCode;

use clipanion::advanced::Cli;
use zpm::commands::YarnCli;

fn main() -> ExitCode {
    YarnCli::run_default()
}
