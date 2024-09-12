use clipanion::cli;

#[cli::command(default)]
pub struct Version {
    #[cli::option("-V,--version", required)]
    version: bool,
}

impl Version {
    #[tokio::main()]
    pub async fn execute(&self) {
        println!("{}", self.cli_info.version);
    }
}
