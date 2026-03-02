use std::collections::HashMap;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use futures::stream::StreamExt;
use futures::SinkExt;
use tokio::io::AsyncBufReadExt;
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio_tungstenite::tungstenite::Message;
use zpm_switch::YARN_SWITCH_PATH_ENV;

use super::ipc::{
    BufferedOutputLine, DaemonMessage, DaemonNotification, DaemonRequest,
    DaemonRequestEnvelope, DaemonResponse, SubscriptionScope, TaskSubscription,
    DAEMON_SERVER_ENV,
};
use zpm_utils::Path;

use crate::error::Error;

type PendingRequests = Arc<Mutex<HashMap<u64, oneshot::Sender<DaemonResponse>>>>;

/// Result of pushing tasks to the daemon
pub struct PushTasksResult {
    /// The directly requested task IDs
    pub task_ids: Vec<String>,
    /// Total number of dependency tasks (excluding target tasks)
    pub dependency_count: usize,
}

/// Handle to a standalone daemon process that can be killed when no longer needed
pub struct StandaloneDaemonHandle {
    pid: u32,
}

impl StandaloneDaemonHandle {
    /// Kill the standalone daemon process
    pub fn kill(&self) {
        #[cfg(unix)]
        {
            // Kill the entire process group
            let _ = std::process::Command::new("kill")
                .arg("-9")
                .arg(format!("-{}", self.pid))
                .status();
        }
    }
}

impl Drop for StandaloneDaemonHandle {
    fn drop(&mut self) {
        self.kill();
    }
}

pub struct DaemonClient {
    /// Channel to send outgoing messages to the writer task
    outgoing_tx: mpsc::UnboundedSender<Message>,
    /// Channel to receive notifications from the reader task
    notification_rx: mpsc::UnboundedReceiver<DaemonNotification>,
    /// Map of pending request IDs to their response channels
    pending_requests: PendingRequests,
    /// Counter for generating unique request IDs
    next_request_id: Arc<AtomicU64>,
    /// Flag to indicate that close() was called (suppresses error messages)
    closing: Arc<AtomicBool>,
}

impl DaemonClient {
    pub async fn connect(project_root: &Path) -> Result<Self, Error> {
        // Check if we already have a daemon URL from the environment
        let url = match std::env::var(DAEMON_SERVER_ENV) {
            Ok(url) => url,
            Err(_) => start_daemon(project_root).await?,
        };
        Self::connect_to_url(&url).await
    }

    /// Start a new standalone daemon that is not registered and will be killed when the handle is dropped
    pub async fn connect_standalone(project_root: &Path) -> Result<(Self, StandaloneDaemonHandle), Error> {
        let (url, pid) = start_standalone_daemon(project_root).await?;
        let client = Self::connect_to_url(&url).await?;
        Ok((client, StandaloneDaemonHandle { pid }))
    }

    pub async fn connect_to_url(url: &str) -> Result<Self, Error> {
        let (ws_stream, _) = tokio_tungstenite::connect_async(url)
            .await
            .map_err(|e| Error::IpcConnectionFailed(e.to_string()))?;

        let (write, read) = ws_stream.split();

        let (outgoing_tx, outgoing_rx) = mpsc::unbounded_channel::<Message>();
        let (notification_tx, notification_rx) = mpsc::unbounded_channel::<DaemonNotification>();
        let pending_requests: PendingRequests = Arc::new(Mutex::new(HashMap::new()));
        let next_request_id = Arc::new(AtomicU64::new(1));
        let closing = Arc::new(AtomicBool::new(false));

        // Spawn the writer task
        let write = Arc::new(Mutex::new(write));
        let write_clone = write.clone();
        tokio::spawn(async move {
            let mut outgoing_rx = outgoing_rx;
            while let Some(msg) = outgoing_rx.recv().await {
                let mut writer = write_clone.lock().await;
                if writer.send(msg).await.is_err() {
                    break;
                }
            }
        });

        // Spawn the reader task
        let pending_for_reader = pending_requests.clone();
        let write_for_reader = write;
        let closing_for_reader = closing.clone();
        tokio::spawn(async move {
            let mut read = read;
            while let Some(msg_result) = read.next().await {
                match msg_result {
                    Ok(Message::Text(text)) => {
                        match serde_json::from_str::<DaemonMessage>(&text) {
                            Ok(DaemonMessage::Response { request_id, response }) => {
                                let mut pending = pending_for_reader.lock().await;
                                if let Some(sender) = pending.remove(&request_id) {
                                    let _ = sender.send(response);
                                }
                            }
                            Ok(DaemonMessage::Notification { notification }) => {
                                let _ = notification_tx.send(notification);
                            }
                            Err(e) => {
                                eprintln!("Failed to parse daemon message: {} - raw: {}", e, text);
                            }
                        }
                    }
                    Ok(Message::Ping(data)) => {
                        let mut writer = write_for_reader.lock().await;
                        let _ = writer.send(Message::Pong(data)).await;
                    }
                    Ok(Message::Close(_)) => break,
                    Ok(_) => {}
                    Err(e) => {
                        // Only print error if we're not intentionally closing
                        if !closing_for_reader.load(Ordering::Relaxed) {
                            eprintln!("WebSocket read error: {}", e);
                        }
                        break;
                    }
                }
            }
        });

        Ok(Self {
            outgoing_tx,
            notification_rx,
            pending_requests,
            next_request_id,
            closing,
        })
    }

    pub async fn send_request(&mut self, request: DaemonRequest) -> Result<DaemonResponse, Error> {
        let request_id = self.next_request_id.fetch_add(1, Ordering::Relaxed);

        let envelope = DaemonRequestEnvelope {
            request_id,
            request,
        };

        let json = serde_json::to_string(&envelope)
            .map_err(|e| Error::IpcError(e.to_string()))?;

        // Create a oneshot channel for the response
        let (response_tx, response_rx) = oneshot::channel();

        // Register the pending request
        {
            let mut pending = self.pending_requests.lock().await;
            pending.insert(request_id, response_tx);
        }

        // Send the request
        self.outgoing_tx
            .send(Message::Text(json.into()))
            .map_err(|e| Error::IpcError(e.to_string()))?;

        // Wait for the response
        response_rx
            .await
            .map_err(|_| Error::IpcError("Connection closed while waiting for response".to_string()))
    }

    pub async fn recv_notification(&mut self) -> Result<DaemonNotification, Error> {
        self.notification_rx
            .recv()
            .await
            .ok_or_else(|| Error::IpcError("Connection closed".to_string()))
    }

    /// Close the WebSocket connection gracefully
    pub fn close(&self) {
        // Set closing flag to suppress error messages from the reader task
        self.closing.store(true, Ordering::Relaxed);
        let _ = self.outgoing_tx.send(Message::Close(None));
    }

    pub async fn push_tasks(
        &mut self,
        tasks: Vec<TaskSubscription>,
        parent_task_id: Option<String>,
        workspace: Option<String>,
        context_id: Option<String>,
    ) -> Result<PushTasksResult, Error> {
        self.push_tasks_with_subscriptions(
            tasks,
            parent_task_id,
            workspace,
            SubscriptionScope::None,
            SubscriptionScope::None,
            context_id,
        )
        .await
    }

    pub async fn push_tasks_with_subscriptions(
        &mut self,
        tasks: Vec<TaskSubscription>,
        parent_task_id: Option<String>,
        workspace: Option<String>,
        output_subscription: SubscriptionScope,
        status_subscription: SubscriptionScope,
        context_id: Option<String>,
    ) -> Result<PushTasksResult, Error> {
        let request = DaemonRequest::PushTasks {
            tasks,
            parent_task_id,
            workspace,
            output_subscription,
            status_subscription,
            context_id,
        };

        match self.send_request(request).await? {
            DaemonResponse::TasksEnqueued { task_ids, dependency_count } => Ok(PushTasksResult {
                task_ids,
                dependency_count,
            }),
            DaemonResponse::Error { message } => Err(Error::TaskPushFailed(message)),
            _ => Err(Error::IpcError("Unexpected response".to_string())),
        }
    }

    pub async fn get_task_output(
        &mut self,
        task_id: &str,
    ) -> Result<Vec<BufferedOutputLine>, Error> {
        let request = DaemonRequest::GetTaskOutput {
            task_id: task_id.to_string(),
        };

        match self.send_request(request).await? {
            DaemonResponse::TaskOutput { lines, .. } => Ok(lines),
            DaemonResponse::Error { message } => Err(Error::IpcError(message)),
            _ => Err(Error::IpcError("Unexpected response".to_string())),
        }
    }
}

async fn start_daemon(project_root: &Path) -> Result<String, Error> {
    let switch_path = std::env::var(YARN_SWITCH_PATH_ENV).map_err(|_| {
        Error::IpcError(
            "This command can only be called within a Yarn Switch context. \
             Please run this command through `yarn` instead of calling the binary directly."
                .to_string(),
        )
    })?;

    let mut cmd = tokio::process::Command::new(&switch_path);
    cmd.args(["switch", "daemon", "--open"])
        .current_dir(project_root.to_path_buf())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .stdin(Stdio::null());

    let mut child = cmd
        .spawn()
        .map_err(|e| Error::IpcError(format!("Failed to start daemon: {}", e)))?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| Error::IpcError("Failed to capture daemon stdout".to_string()))?;

    let mut reader = tokio::io::BufReader::new(stdout).lines();

    let url = tokio::time::timeout(Duration::from_secs(10), reader.next_line())
        .await
        .map_err(|_| Error::IpcError("Timeout waiting for daemon URL".to_string()))?
        .map_err(|e| Error::IpcError(e.to_string()))?
        .ok_or_else(|| Error::IpcError("Daemon closed without printing URL".to_string()))?;

    let _ = child.wait().await;

    Ok(url.trim().to_string())
}

/// Start a standalone daemon directly without using yarn switch
/// Returns the daemon URL and PID for cleanup
async fn start_standalone_daemon(project_root: &Path) -> Result<(String, u32), Error> {
    let current_exe = std::env::current_exe()
        .map_err(|e| Error::IpcError(format!("Failed to get current executable: {}", e)))?;

    let mut cmd = tokio::process::Command::new(current_exe);
    cmd.args(["debug", "daemon"])
        .current_dir(project_root.to_path_buf())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .stdin(Stdio::null());

    #[cfg(unix)]
    cmd.process_group(0);

    let mut child = cmd
        .spawn()
        .map_err(|e| Error::IpcError(format!("Failed to start standalone daemon: {}", e)))?;

    let pid = child.id().ok_or_else(|| Error::IpcError("Failed to get daemon PID".to_string()))?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| Error::IpcError("Failed to capture daemon stdout".to_string()))?;

    let mut reader = tokio::io::BufReader::new(stdout).lines();

    // Read port from stdout
    let port_line = tokio::time::timeout(Duration::from_secs(10), reader.next_line())
        .await
        .map_err(|_| Error::IpcError("Timeout waiting for daemon port".to_string()))?
        .map_err(|e| Error::IpcError(e.to_string()))?
        .ok_or_else(|| Error::IpcError("Daemon closed without printing port".to_string()))?;

    let port: u16 = port_line.trim().parse()
        .map_err(|_| Error::IpcError(format!("Invalid port from daemon: {}", port_line)))?;

    let url = format!("ws://127.0.0.1:{}", port);

    // Wait for daemon to be ready
    let max_attempts = 100;
    let poll_interval = Duration::from_millis(50);

    for _ in 0..max_attempts {
        match tokio_tungstenite::connect_async(&url).await {
            Ok(_) => return Ok((url, pid)),
            Err(_) => tokio::time::sleep(poll_interval).await,
        }
    }

    Err(Error::IpcError("Timeout waiting for daemon to be ready".to_string()))
}
