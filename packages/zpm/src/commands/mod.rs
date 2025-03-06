use std::process::ExitCode;

use clipanion::advanced::Cli;
use zpm_macros::track_time;

mod debug;

mod add;
mod bin;
mod default;
mod dlx;
mod exec;
mod install;
mod node;
mod pack;
mod run;
mod version;
mod workspaces_list;

clipanion::program!(YarnCli, [
    debug::check_ident::CheckIdent,
    debug::check_range::CheckRange,
    debug::check_reference::CheckReference,
    debug::check_semver_version::CheckSemverVersion,

    add::Add,
    bin::BinList,
    bin::Bin,
    default::Default,
    dlx::DlxWithPackages,
    dlx::Dlx,
    exec::Exec,
    install::Install,
    node::Node,
    pack::Pack,
    run::Run,
    version::Version,
    workspaces_list::WorkspacesList,
]);

#[track_time]
pub fn run_default() -> ExitCode {
    YarnCli::run_default()
}
