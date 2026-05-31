extern crate zpm_allocator;

use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    env_logger::init();

    if std::env::var_os("NO_COLOR").is_some() {
        colored::control::set_override(false);
    } else if let Ok(force_color) = std::env::var("FORCE_COLOR") {
        colored::control::set_override(force_color != "0");
    }

    zpm::commands::run_default(None).await
}
