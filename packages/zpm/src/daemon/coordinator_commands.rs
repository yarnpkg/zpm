// ============================================================================
// Coordinator Commands
//
// All operations that modify state go through these commands.
// This ensures serialized access - no races possible.
// ============================================================================

use tokio::sync::{mpsc, oneshot};

use super::coordinator_state::SubscriptionId;
use super::events::Stream;
use super::ipc::{AttachedLongLivedTask, BufferedOutputLine, DaemonNotification, SubscriptionScope, TaskSubscription};

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
        task_id: String,
        pid: u32,
    },

    /// Unregister a PID when a task exits.
    UnregisterPid {
        task_id: String,
        pid: u32,
    },

    // ========================================================================
    // Executor Event Commands (from executor - replaces spawned event task)
    // ========================================================================

    /// Task has started executing.
    TaskStarted {
        task_id: String,
    },

    /// Task produced output.
    TaskOutput {
        task_id: String,
        line: String,
        stream: Stream,
    },

    /// Task failed with an error.
    TaskFailed {
        task_id: String,
        error: String,
    },

    // ========================================================================
    // Query Commands (from handlers)
    // ========================================================================

    /// Get buffered output for a task.
    GetTaskOutput {
        task_id: String,
        response_tx: oneshot::Sender<Vec<BufferedOutputLine>>,
    },

    /// List all long-lived tasks.
    ListLongLivedTasks {
        response_tx: oneshot::Sender<Vec<LongLivedTaskInfo>>,
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
        target_task_ids: Vec<String>,
        dependency_task_ids: Vec<String>,
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

/// Result of pushing tasks to the scheduler.
#[derive(Debug)]
pub struct PushTasksResult {
    /// The directly requested task IDs
    pub task_ids: Vec<String>,
    /// Dependency task IDs (excluding target tasks)
    pub dependency_ids: Vec<String>,
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
    pub task_id: String,
    pub contextual_task_id: String,
    pub warm_up_complete: bool,
    pub started_at_ms: u64,
}

// ============================================================================
// Command Sender Type
// ============================================================================

pub type CommandSender = mpsc::UnboundedSender<CoordinatorCommand>;
