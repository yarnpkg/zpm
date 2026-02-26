use clipanion::cli;

use crate::error::Error;

/// Start a background daemon process.
///
/// This command starts an idle daemon that runs indefinitely until terminated.
///
#[cli::command]
#[cli::path("daemon")]
#[cli::category("General commands")]
pub struct Daemon {}

impl Daemon {
    pub async fn execute(&self) -> Result<(), Error> {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
        }
    }
}
