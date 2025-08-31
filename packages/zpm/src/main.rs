use std::process::ExitCode;

use zpm_config::YarnConfig;

fn main() -> ExitCode {
    let x = YarnConfig::default();

    env_logger::init();

    zpm::commands::run_default()
}
