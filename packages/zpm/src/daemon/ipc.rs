use serde::{Deserialize, Serialize};

pub const DAEMON_BASE_PORT: u16 = 12197;
pub const TASK_CURRENT_ENV: &str = "ZPM_TASK_CURRENT";
pub const DAEMON_SERVER_ENV: &str = "YARN_DAEMON_SERVER";
pub const LONG_LIVED_CONTEXT_ID: &str = "4d84fea4-e0d4-4df6-8190-f312b86968b3";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskSubscription {
    pub name: String,
    pub args: Vec<String>,
}

/// Defines the scope of subscription for notifications
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SubscriptionScope {
    /// No subscription - don't receive these notifications
    None,
    /// Subscribe only to target tasks (the ones directly requested)
    TargetOnly,
    /// Subscribe to all tasks in the dependency tree
    FullTree,
}

/// Envelope for client-to-server requests, includes a correlation ID
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DaemonRequestEnvelope {
    pub request_id: u64,
    pub request: DaemonRequest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum DaemonRequest {
    Ping,
    PushTasks {
        tasks: Vec<TaskSubscription>,
        parent_task_id: Option<String>,
        workspace: Option<String>,
        output_subscription: SubscriptionScope,
        status_subscription: SubscriptionScope,
        /// Context ID for task execution. Required for new tasks, inherited from parent for subtasks.
        context_id: Option<String>,
    },
    GetTaskOutput {
        task_id: String,
    },
    StopTask {
        task_name: String,
        workspace: Option<String>,
    },
    ListLongLivedTasks,
    /// Cancel all tasks in a given context (used for Ctrl+C handling)
    CancelContext {
        context_id: String,
    },
    /// Get internal state statistics (for debugging/testing memory management)
    GetStats,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BufferedOutputLine {
    pub line: String,
    pub stream: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachedLongLivedTask {
    pub task_id: String,
    pub started_at_ms: u64,
}

/// Status of a long-lived task
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum LongLivedTaskStatus {
    /// Task is not running
    Stopped,
    /// Task is running
    Running {
        started_at_ms: u64,
        process_id: Option<u32>,
    },
}

/// Information about a long-lived task
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LongLivedTaskInfo {
    /// The workspace name
    pub workspace: String,
    /// The task name
    pub task_name: String,
    /// Current status
    pub status: LongLivedTaskStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum DaemonResponse {
    Pong,
    TasksEnqueued {
        /// The directly requested task IDs
        task_ids: Vec<String>,
        /// Total number of dependency tasks (excluding target tasks)
        dependency_count: usize,
        /// Long-lived tasks that we attached to (already running)
        attached_long_lived: Vec<AttachedLongLivedTask>,
    },
    TaskOutput {
        task_id: String,
        lines: Vec<BufferedOutputLine>,
    },
    TaskStopped {
        success: bool,
        error: Option<String>,
    },
    LongLivedTaskList {
        tasks: Vec<LongLivedTaskInfo>,
    },
    ContextCancelled {
        cancelled_count: usize,
    },
    Stats {
        /// Number of entries in the tasks HashMap
        tasks_count: usize,
        /// Number of entries in the prepared BTreeMap
        prepared_count: usize,
        /// Number of entries in the subtasks HashMap
        subtasks_count: usize,
        /// Number of entries in the output_buffer HashMap
        output_buffer_count: usize,
        /// Number of entries in the closed_tasks queue
        closed_tasks_count: usize,
    },
    Error {
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum DaemonNotification {
    TaskOutputLine {
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
    TaskCancelled {
        task_id: String,
    },
    TaskWarmUpComplete {
        task_id: String,
    },
}

/// Unified message type for all server-to-client communication.
/// Uses a `kind` discriminator to distinguish responses from notifications.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum DaemonMessage {
    Response {
        request_id: u64,
        response: DaemonResponse,
    },
    Notification {
        notification: DaemonNotification,
    },
}

impl DaemonMessage {
    pub fn response(request_id: u64, response: DaemonResponse) -> Self {
        Self::Response { request_id, response }
    }

    pub fn notification(notification: DaemonNotification) -> Self {
        Self::Notification { notification }
    }
}

pub fn daemon_url(port: u16) -> String {
    format!("ws://127.0.0.1:{}", port)
}
