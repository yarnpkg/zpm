use std::{process::ExitCode, str::FromStr};

use clipanion::{prelude::*, program, Environment};
use zpm_macros::track_time;
use zpm_utils::ExplicitPath;

mod debug;

mod add;
mod bin;
mod config;
mod config_get;
mod config_set;
mod dlx;
mod exec;
mod install;
mod node;
mod pack;
mod remove;
mod run;
mod set_version;
mod up;
mod version;
mod workspaces_list;
mod workspace;

program!(YarnCli, [
    debug::check_ident::CheckIdent,
    debug::check_range::CheckRange,
    debug::check_reference::CheckReference,
    debug::check_semver_version::CheckSemverVersion,

    add::Add,
    bin::BinList,
    bin::Bin,
    config::Config,
    config_get::ConfigGet,
    config_set::ConfigSet,
    dlx::DlxWithPackages,
    dlx::Dlx,
    exec::Exec,
    install::Install,
    set_version::SetVersion,
    node::Node,
    pack::Pack,
    remove::Remove,
    run::Run,
    up::Up,
    version::Version,
    workspaces_list::WorkspacesList,
    workspace::Workspace,
]);

#[track_time]
pub fn run_default() -> ExitCode {
    let mut args = std::env::args()
        .skip(1)
        .collect::<Vec<_>>();

    if let Some(first_arg) = args.first() {
        let explicit_path
            = ExplicitPath::from_str(first_arg);

        if let Ok(explicit_path) = explicit_path {
            explicit_path.raw_path.path.sys_set_current_dir().unwrap();
            args.remove(0);
        }
    }

    let env
        = Environment::default()
            .with_argv(args);

    YarnCli::run(env)
}
