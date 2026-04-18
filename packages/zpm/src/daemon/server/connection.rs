use std::{net::SocketAddr, sync::Arc};

use futures::{SinkExt, stream::StreamExt};
use tokio::io::AsyncReadExt;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::{
    Message,
    handshake::server::{ErrorResponse, Request, Response},
    protocol::{CloseFrame, frame::coding::CloseCode},
};

use crate::project::Project;

use tokio::sync::oneshot;

use super::super::{
    coordinator_commands::{CommandSender, CoordinatorCommand},
    coordinator_state::SubscriptionId,
    handlers::{create_subscription_if_needed, dispatch_request},
    ipc::{
        DaemonMessage, DaemonNotification, DaemonRequest, DaemonRequestEnvelope,
        DaemonResponse, SubscriptionScope,
    },
};

/// Connection context with only immutable data and command channel.
/// All mutable state access goes through commands.
pub struct ConnectionContext {
    pub command_tx: CommandSender,
    pub auth_token: Option<String>,
    pub project: Arc<Project>,
    pub port: u16,
    pub shutdown_notify: Arc<tokio::sync::Notify>,
}

/// Guard that removes subscription when dropped (via command)
struct SubscriptionGuard {
    subscription_id: SubscriptionId,
    command_tx: CommandSender,
}

impl SubscriptionGuard {
    fn new(subscription_id: SubscriptionId, command_tx: CommandSender) -> Self {
        Self {
            subscription_id,
            command_tx,
        }
    }
}

impl Drop for SubscriptionGuard {
    fn drop(&mut self) {
        let _ = self.command_tx.send(CoordinatorCommand::RemoveSubscription {
            subscription_id: self.subscription_id,
        });
    }
}

/// Extract the `token` query parameter from a WebSocket upgrade request URI.
fn extract_token_from_request(request: &Request) -> Option<String> {
    let uri = request.uri();
    let query = uri.query()?;

    url::form_urlencoded::parse(query.as_bytes())
        .find(|(key, _)| key == "token")
        .map(|(_, value)| value.into_owned())
}

/// Check if a peeked HTTP request contains a WebSocket upgrade header.
fn is_websocket_upgrade(buf: &[u8]) -> bool {
    // Look for "Upgrade:" header with "websocket" value (case-insensitive)
    let text = String::from_utf8_lossy(buf);
    let lower = text.to_ascii_lowercase();
    lower.contains("upgrade:") && lower.contains("websocket")
}

/// Serve a static UI asset via a raw HTTP response written to the stream.
async fn serve_http_request(
    stream: &mut tokio::net::TcpStream,
    peeked: &[u8],
    ctx: &ConnectionContext,
) -> Result<(), std::io::Error> {
    use tokio::io::AsyncWriteExt;

    let request_text = String::from_utf8_lossy(peeked);

    // Parse the request line (e.g. "GET /path HTTP/1.1")
    let first_line = request_text.lines().next().unwrap_or("");
    let parts: Vec<&str> = first_line.split_whitespace().collect();
    let raw_path = if parts.len() >= 2 { parts[1] } else { "/" };

    // Strip query string from path
    let path = raw_path.split('?').next().unwrap_or("/");

    // Map "/" to "index.html"
    let asset_path = match path {
        "/" | "" => "index.html",
        p => p.strip_prefix('/').unwrap_or(p),
    };

    let asset = super::get_ui_asset(asset_path)
        .or_else(|| super::get_ui_asset("index.html"));

    if let Some((content_type, data)) = asset {
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
            data.len(),
            content_type
        );
        stream.write_all(response.as_bytes()).await?;
        stream.write_all(data).await?;
    } else {
        let body = b"404 Not Found";
        let response = format!(
            "HTTP/1.1 404 Not Found\r\nContent-Length: {}\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream.write_all(response.as_bytes()).await?;
        stream.write_all(body).await?;
    }

    Ok(())
}

pub async fn handle_connection(
    mut stream: tokio::net::TcpStream,
    addr: SocketAddr,
    ctx: Arc<ConnectionContext>,
) -> Result<(), zpm_switch::Error> {
    // Peek at the first bytes to determine if this is a WebSocket upgrade
    let mut peek_buf = vec![0u8; 4096];
    let n = stream.peek(&mut peek_buf).await.map_err(|e| {
        zpm_switch::Error::SocketReadError(Arc::new(e))
    })?;

    if n == 0 {
        return Ok(());
    }

    let peeked = &peek_buf[..n];

    if !is_websocket_upgrade(peeked) {
        // Consume the peeked bytes for HTTP handling
        let mut buf = vec![0u8; n];
        let _ = stream.read_exact(&mut buf).await;
        if let Err(e) = serve_http_request(&mut stream, &buf, &ctx).await {
            eprintln!("HTTP error from {}: {}", addr, e);
        }
        return Ok(());
    }

    // WebSocket upgrade path — hand the stream to tungstenite
    let provided_token = Arc::new(std::sync::Mutex::new(None::<String>));
    let token_slot = provided_token.clone();

    let ws_stream = tokio_tungstenite::accept_hdr_async(stream, move |request: &Request, response: Response| -> Result<Response, ErrorResponse> {
        *token_slot.lock().unwrap() = extract_token_from_request(request);
        Ok(response)
    })
    .await
    .map_err(|e| {
        zpm_switch::Error::SocketReadError(std::sync::Arc::new(std::io::Error::new(
            std::io::ErrorKind::Other,
            e.to_string(),
        )))
    })?;

    let (mut write, mut read)
        = ws_stream.split();

    // Validate the token after the handshake so the client can receive error
    // messages through the WebSocket protocol rather than an opaque HTTP 403.
    if let Some(ref expected) = ctx.auth_token {
        let provided = provided_token.lock().unwrap().take();
        if provided.as_deref() != Some(expected.as_str()) {
            let error_msg = DaemonMessage::response(
                0,
                DaemonResponse::Error {
                    message: "Invalid or missing auth token".to_string(),
                },
            );

            if let Ok(json) = serde_json::to_string(&error_msg) {
                let _ = write.send(Message::Text(json.into())).await;
            }

            let _ = write.send(Message::Close(Some(CloseFrame {
                code: CloseCode::Policy,
                reason: "Invalid or missing auth token".into(),
            }))).await;

            // Wait for the client's close acknowledgment (with timeout) so the
            // browser has time to process the error message before we drop the
            // TCP connection.
            let _ = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                async {
                    while let Some(Ok(msg)) = read.next().await {
                        if matches!(msg, Message::Close(_)) {
                            break;
                        }
                    }
                },
            ).await;

            return Ok(());
        }
    }

    // Subscription guards - cleaned up when connection drops
    let mut subscription_guards: Vec<SubscriptionGuard> = Vec::new();

    // Notification receivers from subscriptions
    let mut notification_receivers: Vec<mpsc::UnboundedReceiver<DaemonNotification>> = Vec::new();

    // Subscribe to global notifications (e.g. taskfile changes)
    let mut global_rx = {
        let (tx, rx) = oneshot::channel();
        let _ = ctx.command_tx.send(CoordinatorCommand::SubscribeGlobal { response_tx: tx });
        rx.await.ok()
    };

    loop {
        let notification_future
            = poll_notifications(&mut notification_receivers);

        let global_future = async {
            match &mut global_rx {
                Some(rx) => loop {
                    match rx.recv().await {
                        Ok(n) => break Some(n),
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                            // Missed messages; keep receiving to get the latest
                            continue;
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                            break std::future::pending::<Option<DaemonNotification>>().await;
                        }
                    }
                },
                None => std::future::pending::<Option<DaemonNotification>>().await,
            }
        };

        tokio::select! {
            biased;

            // Handle incoming messages
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
                        let envelope: DaemonRequestEnvelope = match serde_json::from_str(&text) {
                            Ok(r) => r,
                            Err(e) => {
                                let error_response = DaemonMessage::response(
                                    0,
                                    DaemonResponse::Error {
                                        message: format!("Invalid request: {}", e),
                                    },
                                );

                                if let Ok(error_json) = serde_json::to_string(&error_response) {
                                    let _ = write.send(Message::Text(error_json.into())).await;
                                }
                                continue;
                            }
                        };

                        let request_id = envelope.request_id;
                        let request = envelope.request;

                        let subscription_id = setup_subscription_if_needed(
                            &request,
                            &ctx.command_tx,
                            &mut subscription_guards,
                            &mut notification_receivers,
                        ).await;

                        let response = dispatch_request(
                            request,
                            subscription_id,
                            &ctx.command_tx,
                            &ctx.project,
                            ctx.port,
                            ctx.auth_token.as_deref(),
                            &ctx.shutdown_notify,
                        )
                        .await;

                        let message
                            = DaemonMessage::response(request_id, response);

                        let response_json = serde_json::to_string(&message)
                            .map_err(|e| zpm_switch::Error::InvalidDaemonMessage(e.to_string()))?;

                        write
                            .send(Message::Text(response_json.into()))
                            .await
                            .map_err(|e| {
                                zpm_switch::Error::SocketWriteError(std::sync::Arc::new(
                                    std::io::Error::new(std::io::ErrorKind::Other, e.to_string()),
                                ))
                            })?;
                    }
                    Message::Close(frame) => {
                        write.send(Message::Close(frame)).await.ok();
                        break;
                    }
                    Message::Ping(data) => {
                        write.send(Message::Pong(data)).await.ok();
                    }
                    _ => {}
                }
            }

            // Handle notifications from subscriptions
            Some(notification) = notification_future => {
                let message
                    = DaemonMessage::notification(notification);

                let notification_json = serde_json::to_string(&message)
                    .map_err(|e| zpm_switch::Error::InvalidDaemonMessage(e.to_string()))?;

                if write.send(Message::Text(notification_json.into())).await.is_err() {
                    break;
                }
            }

            // Handle global notifications (e.g. taskfile changes)
            Some(notification) = global_future => {
                let message
                    = DaemonMessage::notification(notification);

                let notification_json = serde_json::to_string(&message)
                    .map_err(|e| zpm_switch::Error::InvalidDaemonMessage(e.to_string()))?;

                if write.send(Message::Text(notification_json.into())).await.is_err() {
                    break;
                }
            }
        }
    }

    // subscription_guards dropped here, sending RemoveSubscription commands
    Ok(())
}

pub async fn run_accept_loop(
    listener: tokio::net::TcpListener,
    ctx: Arc<ConnectionContext>,
) {
    loop {
        match listener.accept().await {
            Ok((stream, addr)) => {
                let ctx
                    = ctx.clone();

                tokio::spawn(async move {
                    if let Err(e) = handle_connection(stream, addr, ctx).await {
                        eprintln!("Connection error from {}: {}", addr, e);
                    }
                });
            }
            Err(e) => {
                eprintln!("Failed to accept connection: {}", e);
            }
        }
    }
}

/// If the request is a PushTasks with subscriptions, create a subscription
/// and register its guard and receiver. Returns the subscription ID if created.
async fn setup_subscription_if_needed(
    request: &DaemonRequest,
    command_tx: &CommandSender,
    guards: &mut Vec<SubscriptionGuard>,
    receivers: &mut Vec<mpsc::UnboundedReceiver<DaemonNotification>>,
) -> Option<SubscriptionId> {
    let DaemonRequest::PushTasks {
        output_subscription,
        status_subscription,
        context_id,
        ..
    } = request else {
        return None;
    };

    if *output_subscription == SubscriptionScope::None
        && *status_subscription == SubscriptionScope::None
    {
        return None;
    }

    let (sub_id, rx) = create_subscription_if_needed(
        *output_subscription,
        *status_subscription,
        context_id.clone(),
        command_tx,
    ).await?;

    guards.push(SubscriptionGuard::new(sub_id, command_tx.clone()));
    receivers.push(rx);
    Some(sub_id)
}

async fn poll_notifications(
    receivers: &mut Vec<mpsc::UnboundedReceiver<DaemonNotification>>,
) -> Option<DaemonNotification> {
    if receivers.is_empty() {
        std::future::pending::<Option<DaemonNotification>>().await
    } else {
        let mut start = 0usize;
        futures::future::poll_fn(|cx| {
            let mut polled = 0;
            while polled < receivers.len() {
                let i = (start + polled) % receivers.len();
                match receivers[i].poll_recv(cx) {
                    std::task::Poll::Ready(Some(notif)) => {
                        start = (i + 1) % receivers.len();
                        return std::task::Poll::Ready(Some(notif));
                    }
                    std::task::Poll::Ready(None) => {
                        receivers.swap_remove(i);
                        // Don't increment polled: the swapped-in element
                        // now sits at index i and still needs polling.
                    }
                    std::task::Poll::Pending => {
                        polled += 1;
                    }
                }
            }
            if receivers.is_empty() {
                return std::task::Poll::Pending;
            }
            start = (start + 1) % receivers.len();
            std::task::Poll::Pending
        })
        .await
    }
}
