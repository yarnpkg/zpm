use serde::{Deserialize, Serialize};

/// The base port for daemon WebSocket servers
pub const DAEMON_BASE_PORT: u16 = 12197;

/// Messages that can be sent to the daemon
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum DaemonRequest {
    Ping,
}

/// Responses from the daemon
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum DaemonResponse {
    Pong,
}

/// Get the WebSocket URL for a daemon given its port
pub fn daemon_url(port: u16) -> String {
    format!("ws://127.0.0.1:{}", port)
}
