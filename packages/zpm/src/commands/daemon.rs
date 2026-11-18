use clipanion::cli;

use crate::error::Error;

/// Open the Yarn daemon through Yarn Switch
///
/// The daemon lifecycle is managed by Yarn Switch. Running this command directly from a Yarn binary requires the Switch context and otherwise
/// reports an error.
///
/// ## Examples
///
/// Start the daemon of the current project and open its interface
///
/// `yarn daemon`
///
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
