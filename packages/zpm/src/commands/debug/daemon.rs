use std::net::SocketAddr;

use clipanion::cli;
use futures::stream::StreamExt;
use futures::SinkExt;
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;
use zpm_switch::{DaemonRequest, DaemonResponse, Error as SwitchError, DAEMON_BASE_PORT};

use crate::error::Error;

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
        let (listener, port) = Self::bind_to_available_port().await?;

        // Print port to stdout so the spawner can capture it
        println!("{}", port);

        // Accept WebSocket connections
        loop {
            match listener.accept().await {
                Ok((stream, addr)) => {
                    tokio::spawn(async move {
                        if let Err(e) = Self::handle_connection(stream, addr).await {
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
    ) -> Result<(), SwitchError> {
        let ws_stream = tokio_tungstenite::accept_async(stream)
            .await
            .map_err(|e| SwitchError::SocketReadError(std::sync::Arc::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                e.to_string(),
            ))))?;

        let (mut write, mut read) = ws_stream.split();

        while let Some(msg) = read.next().await {
            let msg = match msg {
                Ok(m) => m,
                Err(e) => {
                    eprintln!("WebSocket error from {}: {}", addr, e);
                    break;
                }
            };

            match msg {
                Message::Text(text) => {
                    let request: DaemonRequest = serde_json::from_str(&text)
                        .map_err(|e| SwitchError::InvalidDaemonMessage(e.to_string()))?;

                    let response = Self::handle_request(request);

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

        Ok(())
    }

    fn handle_request(request: DaemonRequest) -> DaemonResponse {
        match request {
            DaemonRequest::Ping => DaemonResponse::Pong,
        }
    }
}
