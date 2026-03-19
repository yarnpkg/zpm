use serde::{Deserialize, Serialize};

use super::scheduler::ContextualTaskId;

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
        task_id: ContextualTaskId,
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
    /// Get the recent task event history
    GetTaskHistory,
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
    pub task_id: ContextualTaskId,
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

/// Observable lifecycle state for task events.
///
/// Regular tasks: `Scheduled → Started → Completed / Failed / Cancelled`
/// Long-lived tasks: `Scheduled → WarmUp → Live → Failed / Completed`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum TaskEventState {
    /// Task was added to the daemon graph.
    Scheduled,
    /// Task process was spawned (regular tasks).
    Started {
        pid: u32,
    },
    /// Long-lived task process was spawned; warm-up period in progress.
    WarmUp {
        pid: u32,
    },
    /// Long-lived task warm-up completed; task is ready to serve.
    Live {
        pid: u32,
    },
    /// Task completed successfully (exit code 0).
    Completed,
    /// Task failed (non-zero exit code or process error).
    Failed {
        #[serde(skip_serializing_if = "Option::is_none")]
        exit_code: Option<i32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        signal: Option<i32>,
    },
    /// Task was cancelled (dependency failure or context cancellation).
    Cancelled,
}

impl std::fmt::Display for TaskEventState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Scheduled => write!(f, "scheduled"),
            Self::Started { pid } => write!(f, "started (pid {})", pid),
            Self::WarmUp { pid } => write!(f, "warm-up (pid {})", pid),
            Self::Live { pid } => write!(f, "live (pid {})", pid),
            Self::Completed => write!(f, "completed"),
            Self::Failed { exit_code, signal } => {
                write!(f, "failed")?;
                if let Some(code) = exit_code {
                    write!(f, " (exit code {})", code)?;
                }
                if let Some(sig) = signal {
                    write!(f, " (signal {})", sig)?;
                }
                Ok(())
            }
            Self::Cancelled => write!(f, "cancelled"),
        }
    }
}

/// A task state change recorded by the coordinator.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskEvent {
    /// Timestamp in milliseconds since the Unix epoch.
    pub date: u64,
    /// The contextual task ID (e.g. `workspace:taskname@context_id`).
    pub contextual_task_id: ContextualTaskId,
    /// The new task state.
    pub state: TaskEventState,
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
        task_ids: Vec<ContextualTaskId>,
        /// Total number of dependency tasks (excluding target tasks)
        dependency_count: usize,
        /// Long-lived tasks that we attached to (already running)
        attached_long_lived: Vec<AttachedLongLivedTask>,
    },
    TaskOutput {
        task_id: ContextualTaskId,
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
    TaskHistory {
        events: Vec<TaskEvent>,
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
        task_id: ContextualTaskId,
        line: String,
        stream: String,
    },
    TaskStarted {
        task_id: ContextualTaskId,
    },
    TaskCompleted {
        task_id: ContextualTaskId,
        exit_code: i32,
        #[serde(skip_serializing_if = "Option::is_none")]
        signal: Option<i32>,
    },
    TaskCancelled {
        task_id: ContextualTaskId,
    },
    TaskWarmUpComplete {
        task_id: ContextualTaskId,
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
