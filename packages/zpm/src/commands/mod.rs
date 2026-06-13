use std::process::ExitCode;

use clipanion::{prelude::*, Environment};
use zpm_switch::{extract_bin_meta, BinMeta};

mod debug;
mod npm;
mod tasks;

mod rc_helpers;

mod add;
mod bin;
mod cache_clear;
mod config;
mod config_get;
mod config_set;
mod config_unset;
mod create;
mod daemon;
mod constraints;
mod dedupe;
mod dlx;
mod exec;
mod explain;
mod info;
mod init;
mod install;
mod link;
mod node;
mod pack;
mod patch_commit;
mod patch;
mod python;
mod rebuild;
mod remove;
mod run;
mod set_resolution;
mod set_version;
mod set_version_from_sources;
mod unlink;
mod unplug;
mod up;
mod version;
mod workspaces_focus;
mod workspaces_foreach;
mod workspaces_list;
mod workspace;
mod why;

#[cli::program(async)]
pub enum YarnCli {
    BenchCleanup(debug::bench::BenchCleanup),
    BenchIter(debug::bench::BenchIter),
    BenchPrepare(debug::bench::BenchPrepare),
    BenchRun(debug::bench::BenchRun),

    CheckDescriptor(debug::check_descriptor::CheckDescriptor),
    CheckIdent(debug::check_ident::CheckIdent),
    CheckLocator(debug::check_locator::CheckLocator),
    CheckRange(debug::check_range::CheckRange),
    CheckReference(debug::check_reference::CheckReference),
    CheckRequirements(debug::check_requirements::CheckRequirements),
    CheckSemverVersion(debug::check_semver_version::CheckSemverVersion),
    Flamegraph(debug::flamegraph::Flamegraph),
    Http(debug::http::Http),
    IterZip(debug::iter_zip::IterZip),
    MockProxy(debug::mock_proxy::MockProxy),
    PrintBranchBase(debug::print_branch_base::PrintBranchBase),
    PrintChangedFiles(debug::print_changed_files::PrintChangedFiles),
    PrintChangedWorkspaces(debug::print_changed_workspaces::PrintChangedWorkspaces),
    PrintHoisting(debug::print_hoisting::PrintHoisting),
    PrintPlatform(debug::print_platform::PrintPlatform),
    ResolveTask(debug::resolve_task::ResolveTask),
    SyncFs(debug::sync_fs::SyncFs),

    Audit(npm::audit::Audit),
    NpmInfo(npm::info::Info),
    Login(npm::login::Login),
    LogoutAll(npm::logout_all::LogoutAll),
    Logout(npm::logout::Logout),
    Publish(npm::publish::Publish),
    Whoami(npm::whoami::Whoami),

    VersionApply(version::apply::VersionApply),
    VersionCheck(version::check::VersionCheck),
    Version(version::immediate::Version),
    VersionDeferred(version::deferred::VersionDeferred),

    Add(add::Add),
    BinList(bin::BinList),
    Bin(bin::Bin),
    CacheClear(cache_clear::CacheClear),
    CacheClear2(cache_clear::CacheClear2),
    Config(config::Config),
    ConfigGet(config_get::ConfigGet),
    Daemon(debug::daemon::Daemon),
    DaemonStub(daemon::DaemonStub),
    ConfigSet(config_set::ConfigSet),
    ConfigUnset(config_unset::ConfigUnset),
    Constraints(constraints::Constraints),
    Create(create::Create),
    Dedupe(dedupe::Dedupe),
    DlxWithPackages(dlx::DlxWithPackages),
    Dlx(dlx::Dlx),
    Exec(exec::Exec),
    ExplainPeerRequirements(explain::peer_requirements::ExplainPeerRequirements),
    Info(info::Info),
    InitWithTemplate(init::InitWithTemplate),
    Init(init::Init),
    Install(install::Install),
    Link(link::Link),
    SetResolution(set_resolution::SetResolution),
    SetVersion(set_version::SetVersion),
    SetVersionFromSources(set_version_from_sources::SetVersionFromSources),
    Node(node::Node),
    Python(python::Python),
    Pack(pack::Pack),
    PatchCommit(patch_commit::PatchCommit),
    Patch(patch::Patch),
    Rebuild(rebuild::Rebuild),
    Remove(remove::Remove),
    RunList(run::RunList),
    Run(run::Run),
    TaskHistory(tasks::history::TaskHistory),
    TaskList(tasks::list::TaskList),
    TaskPush(tasks::push::TaskPush),
    TaskRunInterlaced(tasks::run_interlaced::TaskRunInterlaced),
    TaskRunBuffered(tasks::run_buffered::TaskRunBuffered),
    TaskRunSilentDependencies(tasks::run_silent_dependencies::TaskRunSilentDependencies),
    TaskStats(tasks::stats::TaskStats),
    TaskStop(tasks::stop::TaskStop),
    Unlink(unlink::Unlink),
    Unplug(unplug::Unplug),
    Up(up::Up),
    WorkspacesFocus(workspaces_focus::WorkspacesFocus),
    WorkspacesForeach(workspaces_foreach::WorkspacesForeach),
    WorkspacesList(workspaces_list::WorkspacesList),
    Workspace(workspace::Workspace),
    Why(why::Why),
}

pub async fn run_default(args: Option<Vec<String>>) -> ExitCode {
    let BinMeta {
        cwd,
        args,
        version,
    } = extract_bin_meta(args);

    if let Some(cwd) = cwd {
        if !cwd.fs_exists() {
            cwd.fs_create_dir_all()
                .expect("Failed to create the requested working directory");
        }

        // SAFETY: Configuration happens during startup before any threads are
        // spawned. `sys_set_current_dir_with_pwd` requires single-threaded
        // execution for the `set_var("PWD", ...)` call.
        unsafe { cwd.sys_set_current_dir_with_pwd() }
            .expect("Failed to set current directory");
    }

    // `--version` / `-v` are handled by clipanion's built-in selector
    // (which short-circuits on `args.len() == 1 && matches!(...,
    // "--version" | "-v")` and prints `env.info.version`), so we don't
    // need a manual short-circuit — we just need to feed it the right
    // version via `with_version`.
    let env
        = Environment::default()
            .with_program_name("Yarn Package Manager".to_string())
            .with_binary_name("yarn".to_string())
            .with_version(version)
            .with_argv(args);

    YarnCli::run(env).await
}
