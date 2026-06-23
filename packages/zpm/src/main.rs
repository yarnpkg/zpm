extern crate zpm_allocator;

use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    env_logger::init();
    let otel_guard = zpm::otel::init();

    if std::env::var_os("NO_COLOR").is_some() {
        colored::control::set_override(false);
    } else if let Ok(force_color) = std::env::var("FORCE_COLOR") {
        colored::control::set_override(force_color != "0");
    }

    let exit_code = zpm::commands::run_default(None).await;

    if let Some(otel_guard) = otel_guard {
        otel_guard.shutdown();
    }

    exit_code
}
