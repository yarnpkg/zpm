extern crate zpm_allocator;

use std::process::ExitCode;

mod cache;
mod commands;
mod cwd;
mod errors;
mod http;
mod install;
mod links;
mod manifest;
mod yarn_enums;
mod yarn;

#[tokio::main()]
async fn main() -> ExitCode {
    commands::run_default().await
}
