use clipanion::cli;

use crate::error::Error;

#[cli::command]
#[cli::path("daemon")]
#[cli::category("Daemon management")]
pub struct DaemonStub {
}

impl DaemonStub {
    pub async fn execute(&self) -> Result<(), Error> {
        Err(Error::MissingYarnSwitchContext)
    }
}
