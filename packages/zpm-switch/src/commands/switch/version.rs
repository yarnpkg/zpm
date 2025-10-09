use clipanion::cli;

/// Print the version of the current Yarn Switch binary
#[cli::command]
#[cli::path("switch")]
#[cli::category("Switch commands")]
#[derive(Debug)]
pub struct VersionCommand {
    #[cli::option("-v,--version")]
    version: bool,
}

impl VersionCommand {
    pub async fn execute(&self) {
        println!("{}", self.cli_environment.info.version);
    }
}
