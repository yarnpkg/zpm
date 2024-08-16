use std::process::ExitCode;

use clipanion::advanced::Cli;
use zpm_macros::track_time;

mod add;
mod bin;
mod default;
mod exec;
mod install;
mod node;
mod run;
mod version;

clipanion::program!(YarnCli, [
    add::Add,
    bin::BinList,
    bin::Bin,
    default::Default,
    exec::Exec,
    install::Install,
    node::Node,
    run::Run,
    version::Version,
]);

#[track_time]
pub fn run_default() -> ExitCode {
    YarnCli::run_default()
}
