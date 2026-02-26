use serde::{Deserialize, Serialize};
use zpm_utils::{Hash64, Path, ToFileString};

use crate::errors::Error;

/// Messages that can be sent to the daemon
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum DaemonRequest {
    Ping,
}

/// Responses from the daemon
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum DaemonResponse {
    Pong,
}

/// Get the socket path for a project's daemon
/// Uses the system temp directory to avoid Unix socket path length limits
pub fn socket_path(project_cwd: &Path) -> Result<Path, Error> {
    let hash = Hash64::from_data(project_cwd.to_file_string().as_bytes());

    // Use temp directory for socket to avoid path length limits (SUN_LEN ~108 on macOS)
    let socket_path = Path::try_from(std::env::temp_dir())?
        .with_join_str(format!("yarn-daemon-{}.sock", hash.short()));

    Ok(socket_path)
}
