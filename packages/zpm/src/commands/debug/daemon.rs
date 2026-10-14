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
pub struct Daemon {
    /// Bind the daemon to a specific port instead of auto-selecting one
    #[cli::option("--port")]
    port: Option<u16>,

    /// Require clients to provide this token to connect
    #[cli::option("--auth-token")]
    auth_token: Option<String>,
}

impl Daemon {
    pub async fn execute(&self) -> Result<(), Error> {
        let project
            = Arc::new(Project::new(None).await?);

        // Windows doesn't let users remove a folder that's the working directory
        // of a running process; the daemon must not prevent the project from
        // being deleted (which is also what makes it shut down).
        #[cfg(windows)]
        zpm_utils::Path::temp_root_dir()?.sys_set_current_dir()?;

        run_daemon(project, self.port, self.auth_token.clone()).await
    }
}
