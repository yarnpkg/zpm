mod connection;

use std::net::SocketAddr;
use std::sync::Arc;

use tokio::net::TcpListener;
use super::ipc::DAEMON_BASE_PORT;

pub use connection::{handle_connection, ConnectionContext, OutputBuffer};

use crate::error::Error;

pub async fn bind_to_available_port() -> Result<(TcpListener, u16), Error> {
    for port in DAEMON_BASE_PORT..=DAEMON_BASE_PORT + 100 {
        let addr: SocketAddr = ([127, 0, 0, 1], port).into();
        if let Ok(listener) = TcpListener::bind(addr).await {
            return Ok((listener, port));
        }
    }

    Err(zpm_switch::Error::FailedToBindSocket(std::sync::Arc::new(std::io::Error::new(
        std::io::ErrorKind::AddrInUse,
        format!(
            "Could not bind to any port in range {}-{}",
            DAEMON_BASE_PORT,
            DAEMON_BASE_PORT + 100
        ),
    )))
    .into())
}

pub async fn run_accept_loop(
    listener: TcpListener,
    ctx: Arc<ConnectionContext>,
) {
    loop {
        match listener.accept().await {
            Ok((stream, addr)) => {
                let ctx_clone = ctx.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_connection(stream, addr, ctx_clone).await {
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
