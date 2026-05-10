use std::time::Duration;

use clipanion::cli;
use futures::{SinkExt, stream::StreamExt};
use tokio_tungstenite::tungstenite::Message;

use crate::errors::Error;

use super::switch::daemon_open::{DaemonOpenCommand, build_ws_url};

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

    let port = entry.port;
    let token = entry.auth_token.as_deref();

    tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(
            request_auth_url(port, token),
        )
    }).ok()
}

/// Connect to the daemon via WebSocket, send a GetAuthUrl request, and return the URL.
async fn request_auth_url(port: u16, token: Option<&str>) -> Result<String, Error> {
    let url = build_ws_url(port, token);

    let (mut ws, _) = tokio_tungstenite::connect_async(&url)
        .await
        .map_err(|e| {
            Error::DaemonConnectionFailed(std::sync::Arc::new(std::io::Error::new(
                std::io::ErrorKind::ConnectionRefused,
                e.to_string(),
            )))
        })?;

    let request = serde_json::json!({
        "requestId": 1,
        "request": { "type": "getAuthUrl" }
    });

    ws.send(Message::Text(request.to_string().into()))
        .await
        .map_err(|e| Error::DaemonConnectionFailed(std::sync::Arc::new(std::io::Error::new(
            std::io::ErrorKind::Other,
            e.to_string(),
        ))))?;

    let timeout = Duration::from_secs(5);
    let response = tokio::time::timeout(timeout, async {
        while let Some(Ok(msg)) = ws.next().await {
            if let Message::Text(text) = msg {
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&text) {
                    if parsed.get("kind").and_then(|k| k.as_str()) == Some("response") {
                        if let Some(resp) = parsed.get("response") {
                            if resp.get("type").and_then(|t| t.as_str()) == Some("authUrl") {
                                if let Some(url) = resp.get("url").and_then(|u| u.as_str()) {
                                    return Some(url.to_string());
                                }
                            }
                        }
                    }
                }
            }
        }
        None
    }).await;

    ws.close(None).await.ok();

    match response {
        Ok(Some(url)) => Ok(url),
        _ => Err(Error::DaemonStartTimeout),
    }
}
