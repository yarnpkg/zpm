use std::process::ExitCode;

use clipanion::{prelude::*, Environment};
use zpm_utils::{Path, ToFileString};

use crate::{cwd::set_fake_cwd, yarn::{extract_bin_meta, BinMeta}};

pub mod init;
pub mod proxy;
pub mod switch;

#[cli::program(async)]
enum SwitchExecCli {
    CacheCheckCommand(switch::cache_check::CacheCheckCommand),
    CacheClearCommand(switch::cache_clear::CacheClearCommand),
    CacheInstallCommand(switch::cache_install::CacheInstallCommand),
    CacheListCommand(switch::cache_list::CacheListCommand),
    ClipanionCommandsCommand(switch::clipanion_commands::ClipanionCommandsCommand),
    ExplicitCommand(switch::explicit::ExplicitCommand),
    LinksListCommand(switch::links_list::LinksListCommand),
    LinksClearCommand(switch::links_clear::LinksClearCommand),
    LinkMigrationCommand(switch::link_migration::LinkMigrationCommand),
    LinkCommand(switch::link::LinkCommand),
    PostinstallCommand(switch::postinstall::PostinstallCommand),
    UnlinkCommand(switch::unlink::UnlinkCommand),
    UpdateCommand(switch::update::UpdateCommand),
    VersionCommand(switch::version::VersionCommand),
    WhichCommand(switch::which::WhichCommand),
    ProxyCommand(proxy::ProxyCommand),
    InitCommand(init::InitCommand),
}

pub async fn run_default() -> ExitCode {
    let self_path = Path::current_exe()
        .unwrap()
        .to_file_string();

    std::env::set_var("YARNSW_EXEC_PATH", self_path);

    let BinMeta {
        cwd,
        args,
        version,
    } = extract_bin_meta();

    if let Some(cwd) = cwd {
        set_fake_cwd(cwd);
    }

    let env
        = Environment::default()
            .with_program_name("Yarn Switch".to_string())
            .with_binary_name("yarn".to_string())
            .with_version(version)
            .with_argv(args);

    SwitchExecCli::run(env).await
}
