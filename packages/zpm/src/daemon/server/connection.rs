// ============================================================================
// Connection Handler - Command-Based
//
// All state access goes through commands. No Arc<RwLock> references.
// Subscriptions are created via commands and cleaned up via commands.
// ============================================================================

use std::net::SocketAddr;
use std::sync::Arc;

use futures::stream::StreamExt;
use futures::SinkExt;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

use super::super::coordinator_commands::{CommandSender, CoordinatorCommand};
use super::super::coordinator_state::SubscriptionId;
use super::super::handlers::{create_subscription_if_needed, dispatch_request};
use super::super::ipc::{
    DaemonMessage, DaemonNotification, DaemonRequest, DaemonRequestEnvelope,
    DaemonResponse, SubscriptionScope,
};
use crate::project::Project;

// ============================================================================
// Connection Context
// ============================================================================

/// Connection context with only immutable data and command channel.
/// All mutable state access goes through commands.
pub struct ConnectionContext {
    pub project: Arc<Project>,
    pub command_tx: CommandSender,
}

// ============================================================================
// Subscription Guard
// ============================================================================

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

// ============================================================================
// Connection Handler
// ============================================================================

pub async fn handle_connection(
    stream: tokio::net::TcpStream,
    addr: SocketAddr,
    ctx: Arc<ConnectionContext>,
) -> Result<(), zpm_switch::Error> {
    let ws_stream = tokio_tungstenite::accept_async(stream)
        .await
        .map_err(|e| {
            zpm_switch::Error::SocketReadError(std::sync::Arc::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                e.to_string(),
            )))
        })?;

    let (mut write, mut read) = ws_stream.split();

    // Subscription guards - cleaned up when connection drops
    let mut subscription_guards: Vec<SubscriptionGuard> = Vec::new();

    // Notification receivers from subscriptions
    let mut notification_receivers: Vec<mpsc::UnboundedReceiver<DaemonNotification>> = Vec::new();

    loop {
        let notification_future = poll_notifications(&mut notification_receivers);

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

                        // Create subscription if needed (via command)
                        let subscription_id = if let DaemonRequest::PushTasks {
                            output_subscription,
                            status_subscription,
                            context_id,
                            ..
                        } = &request
                        {
                            if *output_subscription != SubscriptionScope::None
                                || *status_subscription != SubscriptionScope::None
                            {
                                match create_subscription_if_needed(
                                    *output_subscription,
                                    *status_subscription,
                                    context_id.clone(),
                                    &ctx.command_tx,
                                )
                                .await
                                {
                                    Some((sub_id, rx)) => {
                                        let guard = SubscriptionGuard::new(
                                            sub_id,
                                            ctx.command_tx.clone(),
                                        );
                                        subscription_guards.push(guard);
                                        notification_receivers.push(rx);
                                        Some(sub_id)
                                    }
                                    None => None,
                                }
                            } else {
                                None
                            }
                        } else {
                            None
                        };

                        // Dispatch request via commands
                        let response = dispatch_request(
                            request,
                            &ctx.project,
                            subscription_id,
                            &ctx.command_tx,
                        )
                        .await;

                        let message = DaemonMessage::response(request_id, response);

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
                let message = DaemonMessage::notification(notification);

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

// ============================================================================
// Accept Loop
// ============================================================================

pub async fn run_accept_loop(
    listener: tokio::net::TcpListener,
    ctx: Arc<ConnectionContext>,
) {
    loop {
        match listener.accept().await {
            Ok((stream, addr)) => {
                let ctx = ctx.clone();
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

// ============================================================================
// Notification Polling
// ============================================================================

async fn poll_notifications(
    receivers: &mut [mpsc::UnboundedReceiver<DaemonNotification>],
) -> Option<DaemonNotification> {
    if receivers.is_empty() {
        std::future::pending::<Option<DaemonNotification>>().await
    } else {
        futures::future::poll_fn(|cx| {
            for rx in receivers.iter_mut() {
                match rx.poll_recv(cx) {
                    std::task::Poll::Ready(Some(notif)) => {
                        return std::task::Poll::Ready(Some(notif));
                    }
                    std::task::Poll::Ready(None) => {}
                    std::task::Poll::Pending => {}
                }
            }
            std::task::Poll::Pending
        })
        .await
    }
}
