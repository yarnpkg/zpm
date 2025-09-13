use clipanion::cli;

#[cli::command]
#[cli::path("switch")]
#[cli::category("Switch commands")]
#[cli::description("Print the version of the current Yarn Switch binary")]
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
