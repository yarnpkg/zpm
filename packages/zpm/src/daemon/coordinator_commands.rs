// ============================================================================
// Coordinator Commands
//
// All operations that modify state go through these commands.
// This ensures serialized access - no races possible.
// ============================================================================

use tokio::sync::{mpsc, oneshot};

use super::coordinator_state::SubscriptionId;
use super::events::Stream;
use super::ipc::{AttachedLongLivedTask, BufferedOutputLine, DaemonNotification, SubscriptionScope, TaskEvent, TaskSubscription};
use super::scheduler::ContextualTaskId;

// ============================================================================
// Command Types
// ============================================================================

/// Commands sent to the coordinator for serialized execution.
/// ALL state mutations go through here - no direct access to state.
#[derive(Debug)]
pub enum CoordinatorCommand {
    // ========================================================================
    // Task Management Commands (from handlers)
    // ========================================================================

    /// Add new tasks to the scheduler.
    PushTasks {
        tasks: Vec<TaskSubscription>,
        parent_task_id: Option<String>,
        workspace: Option<String>,
        context_id: Option<String>,
        subscription_id: Option<SubscriptionId>,
        response_tx: oneshot::Sender<PushTasksResult>,
    },

    /// Cancel all tasks in a context and kill running processes.
    CancelContext {
        context_id: String,
        response_tx: oneshot::Sender<CancelContextResult>,
    },

    /// Stop a specific long-lived task by name.
    StopTask {
        task_name: String,
        workspace: Option<String>,
        response_tx: oneshot::Sender<StopTaskResult>,
    },

    // ========================================================================
    // Process Management Commands (from executor)
    // ========================================================================

    /// Register a PID for a task that has just spawned.
    RegisterPid {
        task_id: ContextualTaskId,
        pid: u32,
    },

    /// Unregister a PID when a task exits.
    UnregisterPid {
        task_id: ContextualTaskId,
        pid: u32,
    },

    // ========================================================================
    // Executor Event Commands (from executor - replaces spawned event task)
    // ========================================================================

    /// Task has started executing.
    TaskStarted {
        task_id: ContextualTaskId,
        pid: Option<u32>,
    },

    /// Task produced output.
    TaskOutput {
        task_id: ContextualTaskId,
        line: String,
        stream: Stream,
    },

    /// Task completed (success or failure).
    /// Sent AFTER all output has been streamed, ensuring proper ordering.
    TaskCompleted {
        task_id: ContextualTaskId,
        result: TaskCompletionResult,
    },

    /// Long-lived task warm-up period elapsed.
    /// Sent by a spawned timer after LONG_LIVED_WARMUP_MS.
    WarmUpComplete {
        task_id: ContextualTaskId,
        base_task_id: zpm_tasks::TaskId,
    },

    // ========================================================================
    // Query Commands (from handlers)
    // ========================================================================

    /// Get buffered output for a task.
    GetTaskOutput {
        task_id: ContextualTaskId,
        response_tx: oneshot::Sender<Vec<BufferedOutputLine>>,
    },

    /// List all long-lived tasks.
    ListLongLivedTasks {
        response_tx: oneshot::Sender<Vec<LongLivedTaskInfo>>,
    },

    /// Get internal state statistics.
    GetStats {
        response_tx: oneshot::Sender<StatsResult>,
    },

    /// Get the recent task event history.
    GetTaskHistory {
        response_tx: oneshot::Sender<Vec<TaskEvent>>,
    },

    // ========================================================================
    // Subscription Commands (from connection handlers)
    // ========================================================================

    /// Create a new subscription.
    CreateSubscription {
        output_scope: SubscriptionScope,
        status_scope: SubscriptionScope,
        context_id: Option<String>,
        response_tx: oneshot::Sender<(SubscriptionId, mpsc::UnboundedReceiver<DaemonNotification>)>,
    },

    /// Add tasks to an existing subscription.
    AddTasksToSubscription {
        subscription_id: SubscriptionId,
        target_task_ids: Vec<ContextualTaskId>,
        dependency_task_ids: Vec<ContextualTaskId>,
    },

    /// Remove a subscription.
    RemoveSubscription {
        subscription_id: SubscriptionId,
    },

    // ========================================================================
    // Shutdown Command
    // ========================================================================

    /// Request graceful shutdown, returns all PIDs.
    Shutdown {
        response_tx: oneshot::Sender<Vec<u32>>,
    },
}

// ============================================================================
// Response Types
// ============================================================================

/// Result of a task completion from the executor.
#[derive(Debug)]
pub enum TaskCompletionResult {
    /// Task exited with a status code
    Exited(std::process::ExitStatus),
    /// Task failed to execute
    Error(String),
}

/// Result of pushing tasks to the scheduler.
#[derive(Debug)]
pub struct PushTasksResult {
    /// The directly requested task IDs
    pub task_ids: Vec<ContextualTaskId>,
    /// Dependency task IDs (excluding target tasks)
    pub dependency_ids: Vec<ContextualTaskId>,
    /// Long-lived tasks that we attached to (already running)
    pub attached_long_lived: Vec<AttachedLongLivedTask>,
    /// Error message if the operation failed
    pub error: Option<String>,
}

/// Result of cancelling a context.
#[derive(Debug)]
pub struct CancelContextResult {
    /// Number of tasks cancelled
    pub cancelled_count: usize,
}

/// Result of stopping a task.
#[derive(Debug)]
pub struct StopTaskResult {
    pub success: bool,
    pub error: Option<String>,
}

/// Information about a long-lived task.
#[derive(Debug, Clone)]
pub struct LongLivedTaskInfo {
    pub task_id: zpm_tasks::TaskId,
    pub contextual_task_id: ContextualTaskId,
    pub warm_up_complete: bool,
    pub started_at_ms: u64,
    pub process_id: Option<u32>,
}

/// Internal state statistics for debugging/testing.
#[derive(Debug, Clone)]
pub struct StatsResult {
    pub tasks_count: usize,
    pub prepared_count: usize,
    pub subtasks_count: usize,
    pub output_buffer_count: usize,
    pub closed_tasks_count: usize,
}

// ============================================================================
// Command Sender Type
// ============================================================================

pub type CommandSender = mpsc::UnboundedSender<CoordinatorCommand>;
