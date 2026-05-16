extern crate zpm_allocator;

use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    if std::env::var_os("RUST_LOG").is_some() {
        let _ = env_logger::try_init();
    }

    zpm::commands::run_default(None).await
}
