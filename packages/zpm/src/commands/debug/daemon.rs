use std::sync::Arc;

use clipanion::cli;

use crate::daemon::run_daemon;
use crate::error::Error;
use crate::project::Project;

/// Start a background daemon process.
///
/// This command starts an idle daemon that runs indefinitely until terminated.
/// It listens on a WebSocket server for IPC messages.
///
#[cli::command]
#[cli::path("debug", "daemon")]
#[cli::category("Debug commands")]
pub struct Daemon {}

impl Daemon {
    pub async fn execute(&self) -> Result<(), Error> {
        let project = Arc::new(Project::new(None).await?);
        run_daemon(project).await
    }
}
