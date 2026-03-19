use std::{net::SocketAddr, sync::Arc};

use futures::{SinkExt, stream::StreamExt};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

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

    let (mut write, mut read)
        = ws_stream.split();

    // Subscription guards - cleaned up when connection drops
    let mut subscription_guards: Vec<SubscriptionGuard> = Vec::new();

    // Notification receivers from subscriptions
    let mut notification_receivers: Vec<mpsc::UnboundedReceiver<DaemonNotification>> = Vec::new();

    loop {
        let notification_future
            = poll_notifications(&mut notification_receivers);

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
        futures::future::poll_fn(|cx| {
            let mut i = 0;
            while i < receivers.len() {
                match receivers[i].poll_recv(cx) {
                    std::task::Poll::Ready(Some(notif)) => {
                        return std::task::Poll::Ready(Some(notif));
                    }
                    std::task::Poll::Ready(None) => {
                        receivers.swap_remove(i);
                    }
                    std::task::Poll::Pending => {
                        i += 1;
                    }
                }
            }
            std::task::Poll::Pending
        })
        .await
    }
}
