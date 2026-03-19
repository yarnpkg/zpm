use std::collections::HashMap;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use futures::stream::StreamExt;
use futures::SinkExt;
use tokio::io::AsyncBufReadExt;
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio::task::AbortHandle;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use zpm_switch::YARN_SWITCH_PATH_ENV;

use super::coordinator::start_daemon_inline;
use super::ipc::{
    AttachedLongLivedTask, BufferedOutputLine, DaemonMessage, DaemonNotification, DaemonRequest,
    DaemonRequestEnvelope, DaemonResponse, LongLivedTaskInfo, SubscriptionScope, TaskEvent,
    TaskSubscription, DAEMON_SERVER_ENV, daemon_url,
};
use zpm_utils::Path;

use crate::error::Error;
use crate::project::Project;

type PendingRequests = Arc<Mutex<HashMap<u64, oneshot::Sender<DaemonResponse>>>>;

/// Result of pushing tasks to the daemon
pub struct PushTasksResult {
    /// The directly requested task IDs
    pub task_ids: Vec<String>,
    /// Total number of dependency tasks (excluding target tasks)
    pub dependency_count: usize,
    /// Long-lived tasks that we attached to (already running)
    pub attached_long_lived: Vec<AttachedLongLivedTask>,
}

/// Handle to a standalone daemon running in-process that can be aborted when no longer needed
pub struct StandaloneDaemonHandle {
    abort_handle: AbortHandle,
}

impl StandaloneDaemonHandle {
    pub fn abort(&self) {
        self.abort_handle.abort();
    }
}

impl Drop for StandaloneDaemonHandle {
    fn drop(&mut self) {
        self.abort();
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
        let url
            = match std::env::var(DAEMON_SERVER_ENV) {
                Ok(url) => url,
                Err(_) => start_daemon(project_root).await?,
            };

        Self::connect_to_url(&url).await
    }

    /// Start a new standalone daemon in-process that will be aborted when the handle is dropped
    pub async fn connect_standalone(project: Arc<Project>) -> Result<(Self, StandaloneDaemonHandle), Error> {
        let (port_tx, port_rx)
            = oneshot::channel::<u16>();

        let project_clone
            = project.clone();

        let join_handle
            = tokio::spawn(async move {
                if let Err(e) = start_daemon_inline(project_clone, port_tx).await {
                    eprintln!("Standalone daemon error: {}", e);
                }
            });

        let abort_handle
            = join_handle.abort_handle();

        let port
            = port_rx
                .await
                .map_err(|_| Error::IpcError("Daemon failed to start".to_string()))?;

        let url
            = daemon_url(port);

        // Poll until daemon is ready
        let max_attempts
            = 100;

        let poll_interval
            = Duration::from_millis(50);

        for _ in 0..max_attempts {
            match tokio_tungstenite::connect_async(&url).await {
                Ok((ws_stream, _)) => {
                    let client
                        = Self::connect_with_stream(ws_stream);

                    return Ok((client, StandaloneDaemonHandle { abort_handle }));
                }
                Err(_) => tokio::time::sleep(poll_interval).await,
            }
        }

        Err(Error::IpcError("Timeout waiting for daemon to be ready".to_string()))
    }

    pub async fn connect_to_url(url: &str) -> Result<Self, Error> {
        let (ws_stream, _)
            = tokio_tungstenite::connect_async(url)
                .await
                .map_err(|e| Error::IpcConnectionFailed(e.to_string()))?;

        Ok(Self::connect_with_stream(ws_stream))
    }

    pub fn connect_with_stream(ws_stream: WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>) -> Self {
        let (write, read)
            = ws_stream.split();

        let (outgoing_tx, outgoing_rx)
            = mpsc::unbounded_channel::<Message>();

        let (notification_tx, notification_rx)
            = mpsc::unbounded_channel::<DaemonNotification>();

        let pending_requests: PendingRequests
            = Arc::new(Mutex::new(HashMap::new()));

        let next_request_id
            = Arc::new(AtomicU64::new(1));

        let closing
            = Arc::new(AtomicBool::new(false));

        let write
            = Arc::new(Mutex::new(write));

        let write_clone
            = write.clone();

        tokio::spawn(async move {
            let mut outgoing_rx
                = outgoing_rx;

            while let Some(msg) = outgoing_rx.recv().await {
                let mut writer
                    = write_clone.lock().await;

                if writer.send(msg).await.is_err() {
                    break;
                }
            }
        });

        let pending_for_reader
            = pending_requests.clone();

        let write_for_reader
            = write;

        let closing_for_reader
            = closing.clone();

        tokio::spawn(async move {
            let mut read
                = read;

            while let Some(msg_result) = read.next().await {
                match msg_result {
                    Ok(Message::Text(text)) => {
                        match serde_json::from_str::<DaemonMessage>(&text) {
                            Ok(DaemonMessage::Response { request_id, response }) => {
                                let mut pending
                                    = pending_for_reader.lock().await;

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
                        let mut writer
                            = write_for_reader.lock().await;

                        let _ = writer.send(Message::Pong(data)).await;
                    }
                    Ok(Message::Close(_)) => break,
                    Ok(_) => {}
                    Err(e) => {
                        if !closing_for_reader.load(Ordering::Relaxed) {
                            eprintln!("WebSocket read error: {}", e);
                        }
                        break;
                    }
                }
            }
        });

        Self {
            outgoing_tx,
            notification_rx,
            pending_requests,
            next_request_id,
            closing,
        }
    }

    pub async fn send_request(&mut self, request: DaemonRequest) -> Result<DaemonResponse, Error> {
        let request_id
            = self.next_request_id.fetch_add(1, Ordering::Relaxed);

        let envelope
            = DaemonRequestEnvelope {
                request_id,
                request,
            };

        let json
            = serde_json::to_string(&envelope)
                .map_err(|e| Error::IpcError(e.to_string()))?;

        let (response_tx, response_rx)
            = oneshot::channel();

        {
            let mut pending
                = self.pending_requests.lock().await;

            pending.insert(request_id, response_tx);
        }

        self.outgoing_tx
            .send(Message::Text(json.into()))
            .map_err(|e| Error::IpcError(e.to_string()))?;

        const REQUEST_TIMEOUT_SECS: u64 = 30;

        let result = tokio::time::timeout(
            Duration::from_secs(REQUEST_TIMEOUT_SECS),
            response_rx,
        )
        .await;

        match result {
            Err(_) => {
                // Timeout: clean up the stale pending request to prevent unbounded growth
                self.pending_requests.lock().await.remove(&request_id);
                Err(Error::IpcError("Request timed out".to_string()))
            }
            Ok(Err(_)) => Err(Error::IpcError("Connection closed while waiting for response".to_string())),
            Ok(Ok(resp)) => Ok(resp),
        }
    }

    pub async fn recv_notification(&mut self) -> Result<DaemonNotification, Error> {
        self.notification_rx
            .recv()
            .await
            .ok_or_else(|| Error::IpcError("Connection closed".to_string()))
    }

    pub fn close(&self) {
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
        let request
            = DaemonRequest::PushTasks {
                tasks,
                parent_task_id,
                workspace,
                output_subscription,
                status_subscription,
                context_id,
            };

        match self.send_request(request).await? {
            DaemonResponse::TasksEnqueued { task_ids, dependency_count, attached_long_lived } => Ok(PushTasksResult {
                task_ids,
                dependency_count,
                attached_long_lived,
            }),
            DaemonResponse::Error { message } => Err(Error::TaskPushFailed(message)),
            _ => Err(Error::IpcError("Unexpected response".to_string())),
        }
    }

    pub async fn get_task_output(
        &mut self,
        task_id: &str,
    ) -> Result<Vec<BufferedOutputLine>, Error> {
        let request
            = DaemonRequest::GetTaskOutput {
                task_id: task_id.to_string(),
            };

        match self.send_request(request).await? {
            DaemonResponse::TaskOutput { lines, .. } => Ok(lines),
            DaemonResponse::Error { message } => Err(Error::IpcError(message)),
            _ => Err(Error::IpcError("Unexpected response".to_string())),
        }
    }

    pub async fn stop_task(
        &mut self,
        task_name: &str,
        workspace: Option<String>,
    ) -> Result<(bool, Option<String>), Error> {
        let request
            = DaemonRequest::StopTask {
                task_name: task_name.to_string(),
                workspace,
            };

        match self.send_request(request).await? {
            DaemonResponse::TaskStopped { success, error } => Ok((success, error)),
            DaemonResponse::Error { message } => Err(Error::IpcError(message)),
            _ => Err(Error::IpcError("Unexpected response".to_string())),
        }
    }

    pub async fn list_long_lived_tasks(&mut self) -> Result<Vec<LongLivedTaskInfo>, Error> {
        let request
            = DaemonRequest::ListLongLivedTasks;

        match self.send_request(request).await? {
            DaemonResponse::LongLivedTaskList { tasks } => Ok(tasks),
            DaemonResponse::Error { message } => Err(Error::IpcError(message)),
            _ => Err(Error::IpcError("Unexpected response".to_string())),
        }
    }

    pub async fn cancel_context(&mut self, context_id: &str) -> Result<usize, Error> {
        let request
            = DaemonRequest::CancelContext {
                context_id: context_id.to_string(),
            };

        match self.send_request(request).await? {
            DaemonResponse::ContextCancelled { cancelled_count } => Ok(cancelled_count),
            DaemonResponse::Error { message } => Err(Error::IpcError(message)),
            _ => Err(Error::IpcError("Unexpected response".to_string())),
        }
    }

    /// Get internal state statistics from the daemon (for debugging/testing)
    pub async fn get_stats(&mut self) -> Result<DaemonStatsResult, Error> {
        let request = DaemonRequest::GetStats;

        match self.send_request(request).await? {
            DaemonResponse::Stats {
                tasks_count,
                prepared_count,
                subtasks_count,
                output_buffer_count,
                closed_tasks_count,
            } => Ok(DaemonStatsResult {
                tasks_count,
                prepared_count,
                subtasks_count,
                output_buffer_count,
                closed_tasks_count,
            }),
            DaemonResponse::Error { message } => Err(Error::IpcError(message)),
            _ => Err(Error::IpcError("Unexpected response".to_string())),
        }
    }

    /// Get the recent task event history from the daemon
    pub async fn get_task_history(&mut self) -> Result<Vec<TaskEvent>, Error> {
        let request = DaemonRequest::GetTaskHistory;

        match self.send_request(request).await? {
            DaemonResponse::TaskHistory { events } => Ok(events),
            DaemonResponse::Error { message } => Err(Error::IpcError(message)),
            _ => Err(Error::IpcError("Unexpected response".to_string())),
        }
    }
}

/// Result of getting daemon statistics
pub struct DaemonStatsResult {
    pub tasks_count: usize,
    pub prepared_count: usize,
    pub subtasks_count: usize,
    pub output_buffer_count: usize,
    pub closed_tasks_count: usize,
}

async fn start_daemon(project_root: &Path) -> Result<String, Error> {
    let switch_path
        = std::env::var(YARN_SWITCH_PATH_ENV).map_err(|_| {
            Error::IpcError(
                "This command can only be called within a Yarn Switch context. \
                 Please run this command through `yarn` instead of calling the binary directly."
                    .to_string(),
            )
        })?;

    let mut cmd
        = tokio::process::Command::new(&switch_path);

    cmd.args(["switch", "daemon", "--open"])
        .current_dir(project_root.to_path_buf())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .stdin(Stdio::null());

    let mut child
        = cmd
            .spawn()
            .map_err(|e| Error::IpcError(format!("Failed to start daemon: {}", e)))?;

    let stdout
        = child
            .stdout
            .take()
            .ok_or_else(|| Error::IpcError("Failed to capture daemon stdout".to_string()))?;

    let mut reader
        = tokio::io::BufReader::new(stdout).lines();

    let url
        = tokio::time::timeout(Duration::from_secs(10), reader.next_line())
            .await
            .map_err(|_| Error::IpcError("Timeout waiting for daemon URL".to_string()))?
            .map_err(|e| Error::IpcError(e.to_string()))?
            .ok_or_else(|| Error::IpcError("Daemon closed without printing URL".to_string()))?;

    let _ = child.wait().await;

    Ok(url.trim().to_string())
}

