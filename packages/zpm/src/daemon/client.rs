use std::process::Stdio;
use std::time::Duration;

use futures::stream::StreamExt;
use futures::SinkExt;
use tokio::io::AsyncBufReadExt;
use tokio_tungstenite::tungstenite::Message;
use zpm_switch::{daemon_url, DaemonRequest, DaemonResponse, DaemonNotification, TaskSubscription};
use zpm_utils::Path;

use crate::error::Error;
use crate::project::Project;

/// A WebSocket client for communicating with the daemon
pub struct DaemonClient {
    write: futures::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
        Message,
    >,
    read: futures::stream::SplitStream<
        tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    >,
}

impl DaemonClient {
    /// Connect to the daemon, starting it if necessary
    pub async fn connect() -> Result<Self, Error> {
        let url = get_or_start_daemon().await?;
        Self::connect_to_url(&url).await
    }

    /// Connect to a daemon at a specific URL
    pub async fn connect_to_url(url: &str) -> Result<Self, Error> {
        let (ws_stream, _) = tokio_tungstenite::connect_async(url)
            .await
            .map_err(|e| Error::IpcConnectionFailed(e.to_string()))?;

        let (write, read) = ws_stream.split();

        Ok(Self { write, read })
    }

    /// Send a request and receive a response
    pub async fn send_request(&mut self, request: DaemonRequest) -> Result<DaemonResponse, Error> {
        let json = serde_json::to_string(&request)
            .map_err(|e| Error::IpcError(e.to_string()))?;

        self.write
            .send(Message::Text(json.into()))
            .await
            .map_err(|e| Error::IpcError(e.to_string()))?;

        loop {
            match self.read.next().await {
                Some(Ok(Message::Text(text))) => {
                    let response: DaemonResponse = serde_json::from_str(&text)
                        .map_err(|e| Error::IpcError(e.to_string()))?;
                    return Ok(response);
                }
                Some(Ok(Message::Ping(data))) => {
                    self.write
                        .send(Message::Pong(data))
                        .await
                        .map_err(|e| Error::IpcError(e.to_string()))?;
                }
                Some(Ok(_)) => continue,
                Some(Err(e)) => return Err(Error::IpcError(e.to_string())),
                None => return Err(Error::IpcError("Connection closed".to_string())),
            }
        }
    }

    /// Receive a notification
    pub async fn recv_notification(&mut self) -> Result<DaemonNotification, Error> {
        loop {
            match self.read.next().await {
                Some(Ok(Message::Text(text))) => {
                    let notification: DaemonNotification = serde_json::from_str(&text)
                        .map_err(|e| Error::IpcError(e.to_string()))?;
                    return Ok(notification);
                }
                Some(Ok(Message::Ping(data))) => {
                    self.write
                        .send(Message::Pong(data))
                        .await
                        .map_err(|e| Error::IpcError(e.to_string()))?;
                }
                Some(Ok(_)) => continue,
                Some(Err(e)) => return Err(Error::IpcError(e.to_string())),
                None => return Err(Error::IpcError("Connection closed".to_string())),
            }
        }
    }

    /// Push tasks to the daemon
    pub async fn push_tasks(
        &mut self,
        tasks: Vec<TaskSubscription>,
        parent_task_id: Option<String>,
    ) -> Result<Vec<String>, Error> {
        let request = DaemonRequest::PushTasks {
            tasks,
            parent_task_id,
        };

        match self.send_request(request).await? {
            DaemonResponse::TasksEnqueued { task_ids } => Ok(task_ids),
            DaemonResponse::Error { message } => Err(Error::TaskPushFailed(message)),
            _ => Err(Error::IpcError("Unexpected response".to_string())),
        }
    }
}

/// Get the daemon URL, starting it if necessary
async fn get_or_start_daemon() -> Result<String, Error> {
    let project = Project::new(None).await?;
    let project_root = project.project_cwd.clone();

    // Check for existing daemon
    if let Some(existing) = zpm_switch::daemons::get_daemon(&project_root)
        .map_err(|e: zpm_switch::Error| Error::IpcError(e.to_string()))?
    {
        if zpm_switch::daemons::is_process_alive(existing.pid) {
            // Verify daemon is responding
            if ping_daemon(existing.port).await.is_ok() {
                return Ok(daemon_url(existing.port));
            }
        }
        // Clean up stale entry
        let _ = zpm_switch::daemons::unregister_daemon(&project_root);
    }

    // Start new daemon
    start_daemon(&project_root).await
}

/// Start a new daemon process
async fn start_daemon(project_root: &Path) -> Result<String, Error> {
    // Find the yarn binary
    let exe_path = std::env::current_exe()
        .map_err(|e| Error::IpcError(e.to_string()))?;

    let mut child = tokio::process::Command::new(&exe_path)
        .args(["debug", "daemon"])
        .current_dir(project_root.to_path_buf())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .stdin(Stdio::null())
        .spawn()
        .map_err(|e| Error::IpcError(format!("Failed to start daemon: {}", e)))?;

    // Read port from stdout
    let stdout = child.stdout.take()
        .ok_or_else(|| Error::IpcError("Failed to capture daemon stdout".to_string()))?;

    let mut reader = tokio::io::BufReader::new(stdout).lines();

    let port_str = tokio::time::timeout(Duration::from_secs(10), reader.next_line())
        .await
        .map_err(|_| Error::IpcError("Timeout waiting for daemon port".to_string()))?
        .map_err(|e| Error::IpcError(e.to_string()))?
        .ok_or_else(|| Error::IpcError("Daemon closed without printing port".to_string()))?;

    let port: u16 = port_str.trim().parse()
        .map_err(|_| Error::IpcError(format!("Invalid port from daemon: {}", port_str)))?;

    // Wait for daemon to be ready
    for _ in 0..100 {
        if ping_daemon(port).await.is_ok() {
            // Register the daemon
            let entry = zpm_switch::daemons::DaemonEntry {
                project_cwd: project_root.clone(),
                yarn_version: zpm_semver::Version::new(),
                pid: child.id().unwrap_or(0),
                port,
            };
            let _ = zpm_switch::daemons::register_daemon(&entry);

            return Ok(daemon_url(port));
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    Err(Error::IpcError("Daemon failed to start".to_string()))
}

/// Ping the daemon to check if it's alive
async fn ping_daemon(port: u16) -> Result<(), Error> {
    let url = daemon_url(port);

    let (mut ws_stream, _) = tokio_tungstenite::connect_async(&url)
        .await
        .map_err(|e| Error::IpcError(e.to_string()))?;

    let request = serde_json::to_string(&DaemonRequest::Ping)
        .map_err(|e| Error::IpcError(e.to_string()))?;

    ws_stream
        .send(Message::Text(request.into()))
        .await
        .map_err(|e| Error::IpcError(e.to_string()))?;

    match ws_stream.next().await {
        Some(Ok(Message::Text(text))) => {
            let response: DaemonResponse = serde_json::from_str(&text)
                .map_err(|e| Error::IpcError(e.to_string()))?;
            match response {
                DaemonResponse::Pong => Ok(()),
                _ => Err(Error::IpcError("Unexpected response".to_string())),
            }
        }
        _ => Err(Error::IpcError("Failed to ping daemon".to_string())),
    }
}
