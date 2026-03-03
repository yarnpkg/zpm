use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, RwLock};

use futures::stream::StreamExt;
use futures::SinkExt;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

use super::super::handlers::dispatch_request;
use super::super::ipc::{
    BufferedOutputLine, DaemonMessage, DaemonNotification, DaemonRequest, DaemonRequestEnvelope,
    DaemonResponse, SubscriptionScope,
};
use super::super::long_lived::LongLivedRegistry;
use super::super::scheduler::Scheduler;
use super::super::subscriptions::{SubscriptionGuard, SubscriptionRegistry};
use crate::project::Project;

pub type OutputBuffer = Arc<RwLock<HashMap<String, Vec<BufferedOutputLine>>>>;

pub struct ConnectionContext {
    pub project: Arc<Project>,
    pub scheduler: Arc<Scheduler>,
    pub subscription_registry: Arc<SubscriptionRegistry>,
    pub output_buffer: OutputBuffer,
    pub long_lived_registry: Arc<LongLivedRegistry>,
}

pub async fn handle_connection(
    stream: tokio::net::TcpStream,
    addr: SocketAddr,
    ctx: Arc<ConnectionContext>,
) -> Result<(), zpm_switch::Error> {
    let ws_stream
        = tokio_tungstenite::accept_async(stream)
            .await
            .map_err(|e| {
                zpm_switch::Error::SocketReadError(std::sync::Arc::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                )))
            })?;

    let (mut write, mut read)
        = ws_stream.split();

    let mut _subscription_guards: Vec<SubscriptionGuard>
        = Vec::new();

    let mut notification_receivers: Vec<mpsc::UnboundedReceiver<DaemonNotification>>
        = Vec::new();

    loop {
        let notification_future
            = poll_notifications(&mut notification_receivers);

        tokio::select! {
            biased;

            msg_opt = read.next() => {
                let msg
                    = match msg_opt {
                    Some(Ok(m)) => m,
                    Some(Err(e)) => {
                        eprintln!("WebSocket error from {}: {}", addr, e);
                        break;
                    }
                    None => break,
                };

                match msg {
                    Message::Text(text) => {
                        let envelope: DaemonRequestEnvelope
                            = match serde_json::from_str(&text) {
                                Ok(r) => r,
                                Err(e) => {
                                    let error_response
                                        = DaemonMessage::response(
                                            0,
                                            DaemonResponse::Error {
                                                message: format!("Invalid request: {}", e),
                                            },
                                        );

                                    let error_json
                                        = serde_json::to_string(&error_response).unwrap();

                                    write.send(Message::Text(error_json.into())).await.ok();
                                    continue;
                                }
                            };

                        let request_id
                            = envelope.request_id;

                        let request
                            = envelope.request;

                        let subscription_id
                            = if let DaemonRequest::PushTasks {
                                output_subscription,
                                status_subscription,
                                context_id,
                                ..
                            } = &request
                            {
                                if *output_subscription != SubscriptionScope::None
                                    || *status_subscription != SubscriptionScope::None
                                {
                                    let (sub_id, rx)
                                        = ctx.subscription_registry.create_subscription(
                                            *output_subscription,
                                            *status_subscription,
                                            context_id.clone(),
                                        );

                                    let guard
                                        = SubscriptionGuard::new(sub_id, ctx.subscription_registry.clone());

                                    _subscription_guards.push(guard);
                                    notification_receivers.push(rx);
                                    Some(sub_id)
                                } else {
                                    None
                                }
                            } else {
                                None
                            };

                        let response
                            = dispatch_request(
                                request,
                                &ctx.scheduler,
                                &ctx.project,
                                &ctx.output_buffer,
                                &ctx.subscription_registry,
                                &ctx.long_lived_registry,
                                subscription_id,
                            );

                        let message
                            = DaemonMessage::response(request_id, response);

                        let response_json
                            = serde_json::to_string(&message)
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

            Some(notification) = notification_future => {
                let message
                    = DaemonMessage::notification(notification);

                let notification_json
                    = serde_json::to_string(&message)
                        .map_err(|e| zpm_switch::Error::InvalidDaemonMessage(e.to_string()))?;

                if write.send(Message::Text(notification_json.into())).await.is_err() {
                    break;
                }
            }
        }
    }

    Ok(())
}

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
