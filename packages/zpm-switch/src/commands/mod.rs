use std::process::ExitCode;

use clipanion::{prelude::*, program_async, Environment};
use zpm_utils::{Path, ToFileString};

use crate::{cwd::set_fake_cwd, yarn::{extract_bin_meta, BinMeta}};

pub mod init;
pub mod proxy;
pub mod switch;

program_async!(SwitchExecCli, [
    switch::postinstall::PostinstallCommand,
    switch::explicit::ExplicitCommand,
    switch::version::VersionCommand,
    proxy::ProxyCommand,
    init::InitCommand,
]);

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
            .with_program_name("Yarn Switch Wrapper".to_string())
            .with_binary_name("yarn".to_string())
            .with_version(version)
            .with_argv(args);

    SwitchExecCli::run(env).await
}
