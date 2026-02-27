use serde::{Deserialize, Serialize};

/// The base port for daemon WebSocket servers
pub const DAEMON_BASE_PORT: u16 = 12197;

/// Environment variable for the current task ID
pub const TASK_CURRENT_ENV: &str = "ZPM_TASK_CURRENT";

/// A task to be pushed with optional subscriptions
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskSubscription {
    pub name: String,
    #[serde(default)]
    pub subscriptions: Vec<SubscriptionKind>,
}

/// The kinds of notifications a client can subscribe to
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum SubscriptionKind {
    Output,
    Status,
}

/// Messages that can be sent to the daemon
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum DaemonRequest {
    Ping,
    PushTasks {
        tasks: Vec<TaskSubscription>,
        #[serde(default)]
        parent_task_id: Option<String>,
    },
}

/// Responses from the daemon
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum DaemonResponse {
    Pong,
    TasksEnqueued {
        task_ids: Vec<String>,
    },
    Error {
        message: String,
    },
}

/// Server-initiated notifications (not responses to requests)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum DaemonNotification {
    TaskOutput {
        task_id: String,
        line: String,
        stream: String,
    },
    TaskStarted {
        task_id: String,
    },
    TaskCompleted {
        task_id: String,
        exit_code: i32,
    },
    TaskFailed {
        task_id: String,
        error: String,
    },
}

/// Get the WebSocket URL for a daemon given its port
pub fn daemon_url(port: u16) -> String {
    format!("ws://127.0.0.1:{}", port)
}
