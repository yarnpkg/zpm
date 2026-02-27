use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use clipanion::cli;
use futures::stream::StreamExt;
use futures::SinkExt;
use tokio::net::TcpListener;
use tokio::sync::{broadcast, mpsc};
use tokio_tungstenite::tungstenite::Message;
use zpm_switch::{DaemonNotification, DaemonRequest, DaemonResponse, Error as SwitchError, DAEMON_BASE_PORT};
use zpm_utils::{Path, ToFileString};

use crate::daemon::{run_execution_loop, DynamicExecutionState};
use crate::error::Error;
use crate::project::Project;

/// Shared daemon state
struct DaemonState {
    project: Arc<Project>,
    execution_state: Arc<DynamicExecutionState>,
    notification_tx: broadcast::Sender<DaemonNotification>,
}

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
        // Load project at startup
        let project = Arc::new(Project::new(None).await?);

        // Create execution state
        let execution_state = Arc::new(DynamicExecutionState::empty());

        // Create notification broadcast channel
        let (broadcast_tx, _) = broadcast::channel::<DaemonNotification>(1024);

        // Create notification forwarder channel
        let (notification_tx, mut notification_rx) = mpsc::unbounded_channel::<DaemonNotification>();

        // Forward notifications from executor to broadcast channel
        let broadcast_tx_clone = broadcast_tx.clone();
        tokio::spawn(async move {
            while let Some(notification) = notification_rx.recv().await {
                let _ = broadcast_tx_clone.send(notification);
            }
        });

        // Create shared daemon state
        let state = Arc::new(DaemonState {
            project: project.clone(),
            execution_state: execution_state.clone(),
            notification_tx: broadcast_tx,
        });

        // Spawn execution loop
        let exec_project = project.clone();
        let exec_state = execution_state.clone();
        tokio::spawn(async move {
            run_execution_loop(exec_project, exec_state, notification_tx).await;
        });

        // Spawn watchdog to monitor project root
        let project_root = project.project_cwd.clone();
        tokio::spawn(async move {
            Self::watch_project_root(project_root).await;
        });

        let (listener, port) = Self::bind_to_available_port().await?;

        // Print port to stdout so the spawner can capture it
        println!("{}", port);

        // Accept WebSocket connections
        loop {
            match listener.accept().await {
                Ok((stream, addr)) => {
                    let state_clone = state.clone();
                    tokio::spawn(async move {
                        if let Err(e) = Self::handle_connection(stream, addr, state_clone).await {
                            eprintln!("Error handling connection from {}: {}", addr, e);
                        }
                    });
                }
                Err(e) => {
                    eprintln!("Failed to accept connection: {}", e);
                }
            }
        }
    }

    /// Try to bind to ports starting from DAEMON_BASE_PORT until one is available
    async fn bind_to_available_port() -> Result<(TcpListener, u16), Error> {
        for port in DAEMON_BASE_PORT..=DAEMON_BASE_PORT + 100 {
            let addr: SocketAddr = ([127, 0, 0, 1], port).into();
            if let Ok(listener) = TcpListener::bind(addr).await {
                return Ok((listener, port));
            }
        }

        Err(SwitchError::FailedToBindSocket(std::sync::Arc::new(std::io::Error::new(
            std::io::ErrorKind::AddrInUse,
            format!("Could not bind to any port in range {}-{}", DAEMON_BASE_PORT, DAEMON_BASE_PORT + 100),
        ))).into())
    }

    async fn handle_connection(
        stream: tokio::net::TcpStream,
        addr: SocketAddr,
        state: Arc<DaemonState>,
    ) -> Result<(), SwitchError> {
        use std::collections::HashSet;
        use zpm_switch::SubscriptionKind;

        let ws_stream = tokio_tungstenite::accept_async(stream)
            .await
            .map_err(|e| SwitchError::SocketReadError(std::sync::Arc::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                e.to_string(),
            ))))?;

        let (mut write, mut read) = ws_stream.split();

        // Subscribe to the broadcast channel
        let mut broadcast_rx = state.notification_tx.subscribe();

        // Track which task IDs this client is subscribed to
        let mut subscribed_tasks: HashSet<String> = HashSet::new();
        let mut wants_output = false;
        let mut wants_status = false;

        loop {
            tokio::select! {
                // Handle incoming WebSocket messages
                msg_opt = read.next() => {
                    let msg = match msg_opt {
                        Some(Ok(m)) => m,
                        Some(Err(e)) => {
                            eprintln!("WebSocket error from {}: {}", addr, e);
                            break;
                        }
                        None => break,
                    };

                    match msg {
                        Message::Text(text) => {
                            let request: DaemonRequest = match serde_json::from_str(&text) {
                                Ok(r) => r,
                                Err(e) => {
                                    let error_response = DaemonResponse::Error {
                                        message: format!("Invalid request: {}", e),
                                    };
                                    let error_json = serde_json::to_string(&error_response).unwrap();
                                    write.send(Message::Text(error_json.into())).await.ok();
                                    continue;
                                }
                            };

                            // Check for subscriptions in PushTasks
                            if let DaemonRequest::PushTasks { ref tasks, .. } = request {
                                for task_sub in tasks {
                                    if task_sub.subscriptions.contains(&SubscriptionKind::Output) {
                                        wants_output = true;
                                    }
                                    if task_sub.subscriptions.contains(&SubscriptionKind::Status) {
                                        wants_status = true;
                                    }
                                }
                            }

                            let response = Self::handle_request(request, &state);

                            // Track subscribed task IDs from the response
                            if let DaemonResponse::TasksEnqueued { ref task_ids } = response {
                                for task_id in task_ids {
                                    subscribed_tasks.insert(task_id.clone());
                                }
                            }

                            let response_json = serde_json::to_string(&response)
                                .map_err(|e| SwitchError::InvalidDaemonMessage(e.to_string()))?;

                            write.send(Message::Text(response_json.into())).await
                                .map_err(|e| SwitchError::SocketWriteError(std::sync::Arc::new(std::io::Error::new(
                                    std::io::ErrorKind::Other,
                                    e.to_string(),
                                ))))?;
                        }
                        Message::Close(_) => break,
                        Message::Ping(data) => {
                            write.send(Message::Pong(data)).await.ok();
                        }
                        _ => {}
                    }
                }

                // Handle notifications from the broadcast channel
                notification_result = broadcast_rx.recv() => {
                    match notification_result {
                        Ok(notification) => {
                            // For now, forward all notifications to clients that have subscriptions
                            // In the future, we can optimize to only send relevant notifications
                            let should_send = match &notification {
                                DaemonNotification::TaskOutput { .. } => wants_output,
                                DaemonNotification::TaskStarted { .. } => wants_status,
                                DaemonNotification::TaskCompleted { .. } => wants_status,
                                DaemonNotification::TaskFailed { .. } => wants_status,
                            };

                            if should_send {
                                let notification_json = serde_json::to_string(&notification)
                                    .map_err(|e| SwitchError::InvalidDaemonMessage(e.to_string()))?;

                                if write.send(Message::Text(notification_json.into())).await.is_err() {
                                    break;
                                }
                            }
                        }
                        Err(broadcast::error::RecvError::Lagged(_)) => {
                            // Missed some notifications, continue
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            break;
                        }
                    }
                }
            }
        }

        Ok(())
    }

    fn handle_request(request: DaemonRequest, state: &DaemonState) -> DaemonResponse {
        match request {
            DaemonRequest::Ping => DaemonResponse::Pong,
            DaemonRequest::PushTasks { tasks, parent_task_id, workspace } => {
                let mut task_ids = Vec::new();

                for task_sub in &tasks {
                    match state.execution_state.add_pushed_task(
                        state.project.as_ref(),
                        &task_sub.name,
                        parent_task_id.as_deref(),
                        task_sub.args.clone(),
                        workspace.as_deref(),
                    ) {
                        Ok((task_id, _new_count)) => {
                            let task_id_str = format!(
                                "{}:{}",
                                task_id.workspace.to_file_string(),
                                task_id.task_name.as_str()
                            );
                            task_ids.push(task_id_str);
                        }
                        Err(e) => {
                            return DaemonResponse::Error {
                                message: e.to_string(),
                            };
                        }
                    }
                }

                DaemonResponse::TasksEnqueued { task_ids }
            }
        }
    }

    /// Watch the project root directory and exit if it disappears or its inode changes
    async fn watch_project_root(project_root: Path) {
        #[cfg(unix)]
        let initial_inode = {
            use std::os::unix::fs::MetadataExt;
            std::fs::metadata(project_root.to_path_buf())
                .map(|m| m.ino())
                .ok()
        };

        loop {
            tokio::time::sleep(Duration::from_secs(5)).await;

            let path = project_root.to_path_buf();

            // Check if directory still exists
            if !path.exists() {
                eprintln!("Daemon shutting down: project root no longer exists");
                std::process::exit(0);
            }

            // On Unix, also check if inode changed (directory was recreated)
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                if let (Some(initial), Ok(metadata)) = (initial_inode, std::fs::metadata(&path)) {
                    if metadata.ino() != initial {
                        eprintln!("Daemon shutting down: project root inode changed");
                        std::process::exit(0);
                    }
                }
            }
        }
    }
}

