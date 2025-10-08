use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    env_logger::init();

    zpm::commands::run_default().await
}
