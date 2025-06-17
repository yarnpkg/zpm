use std::process::ExitCode;

mod cwd;
mod commands;
mod errors;
mod http;
mod install;
mod manifest;
mod yarn_enums;
mod yarn;

#[tokio::main()]
async fn main() -> ExitCode {
    commands::run_default().await
}
