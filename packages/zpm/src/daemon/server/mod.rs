pub mod connection;
mod ui_assets;

use std::net::SocketAddr;

use tokio::net::TcpListener;

use super::ipc::DAEMON_BASE_PORT;
use crate::error::Error;

pub use ui_assets::get_ui_asset;

pub async fn bind_to_port(port: u16) -> Result<(TcpListener, u16), Error> {
    let addr: SocketAddr = ([127, 0, 0, 1], port).into();
    let listener = TcpListener::bind(addr).await.map_err(|e| {
        zpm_switch::Error::FailedToBindSocket(std::sync::Arc::new(e))
    })?;

    Ok((listener, port))
}

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
