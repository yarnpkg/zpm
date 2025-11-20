use std::process::ExitCode;

use clipanion::{prelude::*, Environment};
use zpm_switch::{extract_bin_meta, BinMeta};

mod debug;
mod npm;

mod add;
mod bin;
mod config;
mod config_get;
mod config_set;
mod constraints;
mod dedupe;
mod dlx;
mod exec;
mod info;
mod init;
mod install;
mod node;
mod pack;
mod patch_commit;
mod patch;
mod rebuild;
mod remove;
mod run;
mod set_resolution;
mod set_version;
mod unplug;
mod up;
mod workspaces_focus;
mod workspaces_list;
mod workspace;
mod why;

#[cli::program(async)]
pub enum YarnCli {
    CheckDescriptor(debug::check_descriptor::CheckDescriptor),
    CheckIdent(debug::check_ident::CheckIdent),
    CheckLocator(debug::check_locator::CheckLocator),
    CheckRange(debug::check_range::CheckRange),
    CheckReference(debug::check_reference::CheckReference),
    CheckRequirements(debug::check_requirements::CheckRequirements),
    CheckSemverVersion(debug::check_semver_version::CheckSemverVersion),
    Http(debug::http::Http),
    IterZip(debug::iter_zip::IterZip),
    PrintHoisting(debug::print_hoisting::PrintHoisting),
    PrintPlatform(debug::print_platform::PrintPlatform),
    SyncFs(debug::sync_fs::SyncFs),

    Login(npm::login::Login),
    LogoutAll(npm::logout_all::LogoutAll),
    Logout(npm::logout::Logout),
    Whoami(npm::whoami::Whoami),

    Add(add::Add),
    BinList(bin::BinList),
    Bin(bin::Bin),
    Config(config::Config),
    ConfigGet(config_get::ConfigGet),
    ConfigSet(config_set::ConfigSet),
    Constraints(constraints::Constraints),
    Dedupe(dedupe::Dedupe),
    DlxWithPackages(dlx::DlxWithPackages),
    Dlx(dlx::Dlx),
    Exec(exec::Exec),
    Info(info::Info),
    InitWithTemplate(init::InitWithTemplate),
    Init(init::Init),
    Install(install::Install),
    SetResolution(set_resolution::SetResolution),
    SetVersion(set_version::SetVersion),
    Node(node::Node),
    Pack(pack::Pack),
    PatchCommit(patch_commit::PatchCommit),
    Patch(patch::Patch),
    Rebuild(rebuild::Rebuild),
    Remove(remove::Remove),
    Run(run::Run),
    Unplug(unplug::Unplug),
    Up(up::Up),
    WorkspacesFocus(workspaces_focus::WorkspacesFocus),
    WorkspacesList(workspaces_list::WorkspacesList),
    Workspace(workspace::Workspace),
    Why(why::Why),
}

pub async fn run_default() -> ExitCode {
    let BinMeta {
        cwd,
        args,
        version,
    } = extract_bin_meta();

    if let Some(cwd) = cwd {
        cwd.sys_set_current_dir()
            .expect("Failed to set current directory");
    }

    let env
        = Environment::default()
            .with_program_name("Yarn Package Manager".to_string())
            .with_binary_name("yarn".to_string())
            .with_version(version)
            .with_argv(args);

    YarnCli::run(env).await
}
