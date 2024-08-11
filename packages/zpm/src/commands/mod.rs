use std::process::ExitCode;

use clipanion::advanced::Cli;
use zpm_macros::track_time;

mod add;
mod bin;
mod exec;
mod install;
mod node;
mod default;
mod run;

clipanion::program!(YarnCli, [
    add::Add,
    bin::BinList,
    bin::Bin,
    exec::Exec,
    install::Install,
    default::Default,
    node::Node,
    run::Run,
]);

#[track_time]
pub fn run_default() -> ExitCode {
    YarnCli::run_default()
}
