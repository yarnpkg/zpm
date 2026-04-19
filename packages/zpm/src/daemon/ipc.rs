use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::scheduler::ContextualTaskId;

pub const DAEMON_BASE_PORT: u16 = 12197;
pub const CURRENT_TASK_ENV_NAME: &str = "YARN_CURRENT_TASK";
pub const DAEMON_SERVER_ENV_NAME: &str = "YARN_DAEMON_SERVER";
pub const LONG_LIVED_CONTEXT_ID: &str = "4d84fea4-e0d4-4df6-8190-f312b86968b3";

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TaskSubscription {
    pub name: String,
    pub args: Vec<String>,
}

/// Defines the scope of subscription for notifications
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, TS)]
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
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DaemonRequestEnvelope {
    #[ts(type = "number")]
    pub request_id: u64,
    pub request: DaemonRequest,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum DaemonRequest {
    Ping,
    GetMeta,
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
        #[ts(type = "string")]
        task_id: ContextualTaskId,
    },
    StopTask {
        task_name: String,
        workspace: Option<String>,
    },
    ListLongLivedTasks,
    /// List all tasks declared in workspace taskfiles.
    ListDeclaredTasks,
    /// Cancel all tasks in a given context (used for Ctrl+C handling)
    CancelContext {
        context_id: String,
    },
    /// Get internal state statistics (for debugging/testing memory management)
    GetStats,
    /// Get the recent task event history
    GetTaskHistory,
    /// Get the HTTP URL for the daemon UI (including auth token)
    GetAuthUrl,
    /// Request graceful daemon shutdown
    Shutdown,
    /// Read a file's content, relative to the project root.
    ReadFile {
        path: String,
    },
    /// Watch a file for changes, relative to the project root.
    WatchFile {
        path: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct BufferedOutputLine {
    pub line: String,
    pub stream: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AttachedLongLivedTask {
    #[ts(type = "string")]
    pub task_id: ContextualTaskId,
    #[ts(type = "number")]
    pub started_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DaemonMeta {
    pub version: String,
    pub cwd: String,
}

/// Status of a long-lived task
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[serde(rename_all = "camelCase")]
pub enum LongLivedTaskStatus {
    /// Task is not running
    Stopped,
    /// Task is running
    Running {
        #[ts(type = "number")]
        started_at_ms: u64,
        process_id: Option<u32>,
    },
}

/// Observable lifecycle state for task events.
///
/// Regular tasks: `Scheduled → Started → Completed / Failed / Cancelled`
/// Long-lived tasks: `Scheduled → WarmUp → Live → Failed / Completed`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
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
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TaskEvent {
    /// Timestamp in milliseconds since the Unix epoch.
    #[ts(type = "number")]
    pub date: u64,
    /// The contextual task ID (e.g. `workspace:taskname@context_id`).
    #[ts(type = "string")]
    pub contextual_task_id: ContextualTaskId,
    /// The new task state.
    pub state: TaskEventState,
}

/// A task declared in a workspace taskfile.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DeclaredTaskInfo {
    pub workspace: String,
    pub task_name: String,
    pub is_long_lived: bool,
}

/// An error encountered while parsing a workspace taskfile.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TaskfileError {
    pub workspace: String,
    pub message: String,
}

/// Information about a long-lived task
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct LongLivedTaskInfo {
    /// The workspace name
    pub workspace: String,
    /// The task name
    pub task_name: String,
    /// Current status
    pub status: LongLivedTaskStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum DaemonResponse {
    Pong,
    Meta {
        version: String,
        cwd: String,
    },
    TasksEnqueued {
        /// The directly requested task IDs
        #[ts(type = "string[]")]
        task_ids: Vec<ContextualTaskId>,
        /// Total number of dependency tasks (excluding target tasks)
        dependency_count: usize,
        /// Long-lived tasks that we attached to (already running)
        attached_long_lived: Vec<AttachedLongLivedTask>,
    },
    TaskOutput {
        #[ts(type = "string")]
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
    DeclaredTaskList {
        tasks: Vec<DeclaredTaskInfo>,
        errors: Vec<TaskfileError>,
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
        /// Number of files being watched for taskfile changes
        watched_files_count: usize,
    },
    AuthUrl {
        url: String,
    },
    ShuttingDown,
    FileContent {
        path: String,
        content: Option<String>,
        encoding: String,
    },
    FileWatched,
    Error {
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum DaemonNotification {
    TaskOutputLine {
        #[ts(type = "string")]
        task_id: ContextualTaskId,
        line: String,
        stream: String,
    },
    TaskStarted {
        #[ts(type = "string")]
        task_id: ContextualTaskId,
    },
    TaskCompleted {
        #[ts(type = "string")]
        task_id: ContextualTaskId,
        exit_code: i32,
        #[serde(skip_serializing_if = "Option::is_none")]
        signal: Option<i32>,
    },
    TaskCancelled {
        #[ts(type = "string")]
        task_id: ContextualTaskId,
    },
    TaskWarmUpComplete {
        #[ts(type = "string")]
        task_id: ContextualTaskId,
    },
    DeclaredTasksChanged {
        tasks: Vec<DeclaredTaskInfo>,
        errors: Vec<TaskfileError>,
    },
    FileChanged {
        path: String,
    },
}

/// Unified message type for all server-to-client communication.
/// Uses a `kind` discriminator to distinguish responses from notifications.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum DaemonMessage {
    Response {
        #[ts(type = "number")]
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
