use std::process::ExitCode;

mod exec;
mod install;
mod node;
mod run;

pub fn run_cli() -> ExitCode {
    clipanion::new![
        exec::Exec,
        install::Install,
        node::Node,
        run::Run,
    ].run_default()
}
