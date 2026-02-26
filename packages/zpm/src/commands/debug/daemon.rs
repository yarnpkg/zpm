use std::sync::Arc;

use clipanion::cli;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use zpm_switch::{socket_path, DaemonRequest, DaemonResponse, Error as SwitchError};
use zpm_utils::Path;

use crate::error::Error;

/// Start a background daemon process.
///
/// This command starts an idle daemon that runs indefinitely until terminated.
/// It listens on a Unix socket for IPC messages.
///
#[cli::command]
#[cli::path("debug", "daemon")]
#[cli::category("Debug commands")]
pub struct Daemon {}

impl Daemon {
    pub async fn execute(&self) -> Result<(), Error> {
        let project_cwd = Path::current_dir()?;
        let sock_path = socket_path(&project_cwd)?;

        // Remove stale socket if it exists
        let _ = sock_path.fs_rm();

        // Ensure parent directory exists
        sock_path.fs_create_parent()?;

        let listener = UnixListener::bind(sock_path.to_path_buf())
            .map_err(|e| SwitchError::FailedToBindSocket(Arc::new(e)))?;

        loop {
            match listener.accept().await {
                Ok((stream, _)) => {
                    tokio::spawn(async move {
                        if let Err(e) = Self::handle_connection(stream).await {
                            eprintln!("Error handling connection: {}", e);
                        }
                    });
                }
                Err(e) => {
                    eprintln!("Failed to accept connection: {}", e);
                }
            }
        }
    }

    async fn handle_connection(stream: tokio::net::UnixStream) -> Result<(), SwitchError> {
        let (reader, mut writer) = stream.into_split();
        let mut reader = BufReader::new(reader);
        let mut line = String::new();

        while reader.read_line(&mut line).await.map_err(|e| SwitchError::SocketReadError(Arc::new(e)))? > 0 {
            let request: DaemonRequest = serde_json::from_str(&line)
                .map_err(|e| SwitchError::InvalidDaemonMessage(e.to_string()))?;

            let response = Self::handle_request(request);

            let response_json = serde_json::to_string(&response)
                .map_err(|e| SwitchError::InvalidDaemonMessage(e.to_string()))?;

            writer.write_all(response_json.as_bytes()).await
                .map_err(|e| SwitchError::SocketWriteError(Arc::new(e)))?;
            writer.write_all(b"\n").await
                .map_err(|e| SwitchError::SocketWriteError(Arc::new(e)))?;
            writer.flush().await
                .map_err(|e| SwitchError::SocketWriteError(Arc::new(e)))?;

            line.clear();
        }

        Ok(())
    }

    fn handle_request(request: DaemonRequest) -> DaemonResponse {
        match request {
            DaemonRequest::Ping => DaemonResponse::Pong,
        }
    }
}
