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
mod constraints;
mod dlx;
mod exec;
mod install;
mod node;
mod pack;
mod remove;
mod run;
mod set_version;
mod up;
mod workspaces_list;
mod workspace;

program!(YarnCli, [
    debug::check_descriptor::CheckDescriptor,
    debug::check_ident::CheckIdent,
    debug::check_range::CheckRange,
    debug::check_reference::CheckReference,
    debug::check_semver_version::CheckSemverVersion,
    debug::print_platform::PrintPlatform,

    add::Add,
    bin::BinList,
    bin::Bin,
    config::Config,
    config_get::ConfigGet,
    config_set::ConfigSet,
    constraints::Constraints,
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

    let mut version_str
        = env!("CARGO_PKG_VERSION").to_string();

    let git_date
        = option_env!("INFRA_GIT_DATE");
    let git_sha
        = option_env!("INFRA_GIT_SHA");

    if let (Some(date), Some(sha)) = (git_date, git_sha) {
        let mut version
            = zpm_semver::Version::from_str(&version_str).unwrap();

        version.rc = Some(vec![
            zpm_semver::VersionRc::String("git".to_string()),
            zpm_semver::VersionRc::Number(date.parse::<u32>().unwrap()),
            zpm_semver::VersionRc::String(format!("hash-{}", sha.to_string())),
        ]);

        version_str
            = version.to_string();
    }

    let env
        = Environment::default()
            .with_program_name("Yarn Package Manager".to_string())
            .with_binary_name("yarn".to_string())
            .with_version(version_str)
            .with_argv(args);

    YarnCli::run(env)
}
