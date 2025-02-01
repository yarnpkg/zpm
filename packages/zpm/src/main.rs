use std::process::ExitCode;

fn main() -> ExitCode {
    if std::env::var("YES_I_KNOW_THIS_IS_EXPERIMENTAL").is_ok() {
        zpm::commands::run_default()
    } else {
        ExitCode::SUCCESS
    }
}
