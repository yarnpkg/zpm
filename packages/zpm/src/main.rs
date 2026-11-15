extern crate zpm_allocator;

use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    #[cfg(windows)]
    zpm_utils::windows_disable_std_handles_inheritance();

    env_logger::init();
    let otel_guard = zpm::otel::init();

    if std::env::var_os("NO_COLOR").is_some() {
        colored::control::set_override(false);
    } else if let Ok(force_color) = std::env::var("FORCE_COLOR") {
        colored::control::set_override(force_color != "0");
    }

    let exit_code = zpm::commands::run_default(None).await;

    // Lets the tests check that expected errors (files probed before being
    // created, etc) don't pay for a backtrace when RUST_BACKTRACE is set
    if std::env::var_os("YARN_IS_TEST_ENV").is_some() && std::env::var_os("YARN_TEST_REPORT_BACKTRACES").is_some() {
        eprintln!("Captured backtraces: {}", zpm_utils::captured_backtrace_count());
    }

    if let Some(otel_guard) = otel_guard {
        otel_guard.shutdown();
    }

    exit_code
}
