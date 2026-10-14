extern crate zpm_allocator;

use std::process::ExitCode;

mod cache;
mod commands;
mod config;
mod cwd;
mod daemons;
mod errors;
mod http;
mod install;
mod ipc;
mod links;
mod manifest;
mod yarn_enums;
mod yarn;

#[tokio::main()]
async fn main() -> ExitCode {
    #[cfg(windows)]
    zpm_utils::windows_disable_std_handles_inheritance();

    commands::run_default().await
}
