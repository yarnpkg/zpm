use std::time::Duration;

use clipanion::cli;

use crate::errors::Error;
use crate::ipc::send_daemon_request;

use super::switch::daemon_open::DaemonOpenCommand;

/// Start or open the daemon for the current project
///
/// This command ensures the selected Yarn daemon is running for the current project, then opens its authentication URL when one is available.
///
#[cli::command]
#[cli::path("daemon")]
#[cli::category("Daemon management")]
pub struct DaemonCommand {
}

impl DaemonCommand {
    pub async fn execute(&self) -> Result<(), Error> {
        let daemon_open
            = DaemonOpenCommand::new(&self.cli_environment);

        daemon_open.execute().await?;

        let url = match request_auth_url_from_registry() {
            Some(url) => url,
            None => return Ok(()),
        };

        if open::that(&url).is_err() {
            eprintln!("Open this URL in your browser: {}", url);
        }

        Ok(())
    }
}

/// Look up the daemon from the registry and request its HTTP auth URL.
fn request_auth_url_from_registry() -> Option<String> {
    let project_cwd
        = crate::cwd::get_final_cwd().ok()?;

    let find_result
        = crate::manifest::find_closest_package_manager(&project_cwd).ok()?;

    let detected_root
        = find_result.detected_root_path?;

    let entry
        = crate::daemons::get_daemon(&detected_root).ok()??;

    tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(
            request_auth_url(&entry),
        )
    }).ok()
}

/// Send a getAuthUrl request to the daemon and return the URL string.
async fn request_auth_url(entry: &crate::daemons::DaemonEntry) -> Result<String, Error> {
    let resp = send_daemon_request(
        entry,
        serde_json::json!({ "type": "getAuthUrl" }),
        Duration::from_secs(5),
    ).await?;

    if resp.get("type").and_then(|t| t.as_str()) == Some("authUrl") {
        if let Some(url) = resp.get("url").and_then(|u| u.as_str()) {
            return Ok(url.to_string());
        }
    }

    Err(Error::InvalidDaemonMessage("expected authUrl response".to_string()))
}
