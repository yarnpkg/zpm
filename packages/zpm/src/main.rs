use std::process::ExitCode;

fn main() -> ExitCode {
    env_logger::init();

    zpm::commands::run_default()
}
