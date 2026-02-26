use std::sync::Arc;

use clipanion::cli;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

use crate::{
    cwd::get_final_cwd,
    errors::Error,
    ipc::{socket_path, DaemonRequest, DaemonResponse},
    manifest::find_closest_package_manager,
};

/// Send a message to the daemon for the current project
#[cli::command]
#[cli::path("switch", "daemon")]
#[cli::category("Daemon management")]
#[derive(Debug)]
pub struct DaemonSendCommand {
    /// JSON message to send to the daemon
    #[cli::option("--send")]
    message: String,
}

impl DaemonSendCommand {
    pub async fn execute(&self) -> Result<(), Error> {
        let project_cwd = get_final_cwd()?;

        let find_result = find_closest_package_manager(&project_cwd)?;

        let detected_root = find_result
            .detected_root_path
            .ok_or(Error::NoProjectFound)?;

        let sock_path = socket_path(&detected_root)?;

        if !sock_path.fs_exists() {
            return Err(Error::DaemonNotRunning);
        }

        let stream: UnixStream = UnixStream::connect(sock_path.to_path_buf())
            .await
            .map_err(|e| Error::DaemonConnectionFailed(Arc::new(e)))?;

        let (reader, mut writer) = stream.into_split();
        let mut reader = BufReader::new(reader);

        // Parse the input JSON to validate it
        let request: DaemonRequest = serde_json::from_str(&self.message)
            .map_err(|e| Error::InvalidDaemonMessage(e.to_string()))?;

        // Send the message
        let request_json = serde_json::to_string(&request)
            .map_err(|e| Error::InvalidDaemonMessage(e.to_string()))?;

        AsyncWriteExt::write_all(&mut writer, request_json.as_bytes()).await
            .map_err(|e| Error::DaemonConnectionFailed(Arc::new(e)))?;
        AsyncWriteExt::write_all(&mut writer, b"\n").await
            .map_err(|e| Error::DaemonConnectionFailed(Arc::new(e)))?;
        AsyncWriteExt::flush(&mut writer).await
            .map_err(|e| Error::DaemonConnectionFailed(Arc::new(e)))?;

        // Read the response
        let mut response_line = String::new();
        reader.read_line(&mut response_line).await
            .map_err(|e| Error::DaemonConnectionFailed(Arc::new(e)))?;

        let response: DaemonResponse = serde_json::from_str(&response_line)
            .map_err(|e| Error::InvalidDaemonMessage(e.to_string()))?;

        // Output the response as JSON
        println!("{}", serde_json::to_string_pretty(&response)
            .map_err(|e| Error::InvalidDaemonMessage(e.to_string()))?);

        Ok(())
    }
}
