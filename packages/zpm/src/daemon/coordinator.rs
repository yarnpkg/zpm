use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::os::unix::fs::MetadataExt;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use tokio::sync::{mpsc, oneshot};
use super::ipc::{daemon_url, AttachedLongLivedTask, BufferedOutputLine, DaemonNotification, TaskSubscription};
use super::process_registry::ProcessRegistry;
use zpm_utils::Path;

use super::events::ExecutorEvent;
use super::executor::ExecutorPool;
use super::long_lived::LongLivedRegistry;
use super::scheduler::{format_contextual_task_id, ContextualTaskId, Scheduler};
use super::server::{bind_to_available_port, run_accept_loop, ConnectionContext, OutputBuffer};
use super::subscriptions::SubscriptionRegistry;
use crate::error::Error;
use crate::project::Project;

use zpm_primitives::Ident;
use zpm_tasks::{TaskId, TaskName};

const LONG_LIVED_WARMUP_MS: u64 = 500;

// ============================================================================
// Coordinator Command Types
// ============================================================================

/// Commands sent from handlers to the coordinator for serialized execution.
/// This eliminates race conditions by ensuring all state mutations happen
/// in a single async task.
#[derive(Debug)]
pub enum CoordinatorCommand {
    /// Add new tasks to the scheduler.
    PushTasks {
        tasks: Vec<TaskSubscription>,
        parent_task_id: Option<String>,
        workspace: Option<String>,
        context_id: Option<String>,
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

    /// Register a PID for a task that has just spawned.
    /// Sent by TaskRunner after spawn_script() returns.
    RegisterPid {
        task_id: String,
        pid: u32,
    },

    /// Unregister a PID when a task exits.
    /// Sent by TaskRunner when process exits.
    UnregisterPid {
        task_id: String,
        pid: u32,
    },
}

/// Result of pushing tasks to the scheduler.
#[derive(Debug)]
pub struct PushTasksResult {
    /// The directly requested task IDs
    pub task_ids: Vec<String>,
    /// Total number of dependency tasks (excluding target tasks)
    pub dependency_count: usize,
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

// ============================================================================
// Spawning Tasks State
// ============================================================================

/// Entry for a task that is currently spawning (between spawn() and PID registration).
#[derive(Debug)]
struct SpawningEntry {
    /// When the spawn was initiated (for debugging/metrics)
    #[allow(dead_code)]
    spawned_at: Instant,
    /// If true, kill the process when the PID arrives
    pending_cancel: bool,
}

/// Tracks tasks currently being spawned (no PID yet).
/// This state is owned exclusively by the coordinator to eliminate race conditions.
#[derive(Debug, Default)]
struct SpawningTasks {
    tasks: HashMap<String, SpawningEntry>,
}

impl SpawningTasks {
    fn new() -> Self {
        Self::default()
    }

    /// Mark a task as spawning (called before executor_pool.spawn()).
    fn mark_spawning(&mut self, task_id: String) {
        self.tasks.insert(task_id, SpawningEntry {
            spawned_at: Instant::now(),
            pending_cancel: false,
        });
    }

    /// Mark a spawning task for cancellation. Returns true if the task was found.
    fn mark_pending_cancel(&mut self, task_id: &str) -> bool {
        if let Some(entry) = self.tasks.get_mut(task_id) {
            entry.pending_cancel = true;
            true
        } else {
            false
        }
    }

    /// Remove and return a spawning entry (called when PID arrives).
    fn take(&mut self, task_id: &str) -> Option<SpawningEntry> {
        self.tasks.remove(task_id)
    }

    /// Get all spawning task IDs for a given context.
    fn get_spawning_for_context(&self, context_id: &str) -> Vec<String> {
        let suffix = format!("@{}", context_id);
        self.tasks.keys()
            .filter(|id| id.ends_with(&suffix))
            .cloned()
            .collect()
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Kill a process group. Uses SIGTERM first.
#[cfg(unix)]
fn kill_process_group(pid: u32) {
    let result = unsafe { libc::killpg(pid as i32, libc::SIGTERM) };
    if result != 0 {
        // If killpg fails (e.g., group doesn't exist), try killing the process directly
        let _ = unsafe { libc::kill(pid as i32, libc::SIGTERM) };
    }
}

#[cfg(not(unix))]
fn kill_process_group(_pid: u32) {
    // No-op on non-Unix platforms
}

// ============================================================================
// Command Handler
// ============================================================================

/// Handle a command from a handler. This function runs in the coordinator loop
/// and has exclusive access to coordinator-owned state.
async fn handle_coordinator_command(
    cmd: CoordinatorCommand,
    scheduler: &Scheduler,
    process_registry: &ProcessRegistry,
    spawning_tasks: &mut SpawningTasks,
    subscription_registry: &SubscriptionRegistry,
    project: &Project,
    long_lived_registry: &LongLivedRegistry,
) {
    match cmd {
        CoordinatorCommand::CancelContext { context_id, response_tx } => {
            // Step 1: Mark all tasks as failed in scheduler
            let cancelled_ids = scheduler.cancel_context(&context_id);

            // Step 2: Get and kill registered PIDs
            let pids = process_registry.take_pids_for_context(&context_id);
            for pid in &pids {
                kill_process_group(*pid);
            }

            // Step 3: Mark spawning tasks for deferred kill
            let spawning_ids = spawning_tasks.get_spawning_for_context(&context_id);
            for task_id in &spawning_ids {
                spawning_tasks.mark_pending_cancel(task_id);
            }

            let _ = response_tx.send(CancelContextResult {
                cancelled_count: cancelled_ids.len() + spawning_ids.len(),
            });
        }

        CoordinatorCommand::StopTask { task_name, workspace, response_tx } => {
            // Build the task ID
            let task_id = match build_task_id_for_stop(&task_name, workspace.as_deref(), project) {
                Some(tid) => tid,
                None => {
                    let _ = response_tx.send(StopTaskResult {
                        success: false,
                        error: Some(format!("Could not resolve task: {}", task_name)),
                    });
                    return;
                }
            };

            // Check if task exists in long-lived registry
            let entry = match long_lived_registry.get_existing(&task_id) {
                Some(e) => e,
                None => {
                    let _ = response_tx.send(StopTaskResult {
                        success: false,
                        error: Some(format!("No running long-lived task found: {}", task_name)),
                    });
                    return;
                }
            };

            // Check if task is in spawning state
            if spawning_tasks.mark_pending_cancel(&entry.contextual_task_id) {
                long_lived_registry.remove(&task_id);
                let _ = response_tx.send(StopTaskResult {
                    success: true,
                    error: Some("Task is spawning, will be killed shortly".to_string()),
                });
                return;
            }

            // Try to take the PID atomically
            if let Some(pid) = process_registry.take_pid_for_task(&entry.contextual_task_id) {
                kill_process_group(pid);
                long_lived_registry.remove(&task_id);
                let _ = response_tx.send(StopTaskResult {
                    success: true,
                    error: None,
                });
            } else if entry.process_id.is_some() {
                // Task completed naturally between lookup and stop
                long_lived_registry.remove(&task_id);
                let _ = response_tx.send(StopTaskResult {
                    success: true,
                    error: Some("Task already completed before stop request was processed".to_string()),
                });
            } else {
                // No PID was ever recorded
                long_lived_registry.remove(&task_id);
                let _ = response_tx.send(StopTaskResult {
                    success: true,
                    error: Some("Task had no process ID, removed from registry".to_string()),
                });
            }
        }

        CoordinatorCommand::PushTasks { tasks, parent_task_id, workspace, context_id, response_tx } => {
            // Delegate to the existing push_tasks logic
            // For now, use the existing handler logic via the scheduler
            let result = execute_push_tasks(
                &tasks,
                parent_task_id.as_deref(),
                workspace.as_deref(),
                context_id.as_deref(),
                scheduler,
                project,
                long_lived_registry,
                subscription_registry,
            ).await;

            let _ = response_tx.send(result);
        }

        CoordinatorCommand::RegisterPid { task_id, pid } => {
            // Check if this task was cancelled while spawning
            if let Some(entry) = spawning_tasks.take(&task_id) {
                if entry.pending_cancel {
                    // Task was cancelled while spawning - kill immediately
                    kill_process_group(pid);
                    return;
                }
            }

            // Normal registration
            process_registry.register_with_task(pid, task_id);
        }

        CoordinatorCommand::UnregisterPid { task_id, pid } => {
            // Clean up spawning state if still present (shouldn't happen normally)
            spawning_tasks.take(&task_id);
            process_registry.unregister_with_task(pid, &task_id);
        }
    }
}

/// Build a TaskId for stop_task command
fn build_task_id_for_stop(task_name: &str, workspace: Option<&str>, project: &Project) -> Option<TaskId> {
    let task_name = TaskName::new(task_name).ok()?;

    let workspace = if let Some(ws_name) = workspace {
        let ident = Ident::new(ws_name);
        project.workspace_by_ident(&ident).ok()?.name.clone()
    } else {
        project.active_workspace().ok()?.name.clone()
    };

    Some(TaskId { workspace, task_name })
}

/// Execute push_tasks logic within the coordinator
async fn execute_push_tasks(
    tasks: &[TaskSubscription],
    parent_task_id: Option<&str>,
    workspace: Option<&str>,
    context_id: Option<&str>,
    scheduler: &Scheduler,
    project: &Project,
    long_lived_registry: &LongLivedRegistry,
    _subscription_registry: &SubscriptionRegistry,
) -> PushTasksResult {
    use super::ipc::LONG_LIVED_CONTEXT_ID;
    use std::time::SystemTime;

    let mut task_ids = Vec::new();
    let mut total_dependency_count = 0;
    let mut attached_long_lived = Vec::new();

    for task_sub in tasks {
        let task_id = build_task_id_for_push(&task_sub.name, workspace, project);

        let is_long_lived = task_id
            .as_ref()
            .and_then(|tid| check_if_long_lived(project, tid))
            .unwrap_or(false);

        // For long-lived tasks, use atomic check-and-claim
        if is_long_lived {
            if let Some(ref tid) = task_id {
                // Check if already running
                if let Some(existing) = long_lived_registry.get_existing(tid) {
                    if !existing.contextual_task_id.is_empty() {
                        task_ids.push(existing.contextual_task_id.clone());

                        let started_at_ms = existing
                            .started_at
                            .duration_since(SystemTime::UNIX_EPOCH)
                            .map(|d| d.as_millis() as u64)
                            .unwrap_or(0);

                        attached_long_lived.push(AttachedLongLivedTask {
                            task_id: existing.contextual_task_id.clone(),
                            started_at_ms,
                        });

                        continue;
                    }
                }
            }
        }

        let effective_context_id = if is_long_lived {
            Some(LONG_LIVED_CONTEXT_ID)
        } else {
            context_id
        };

        match scheduler.add_task(
            project,
            &task_sub.name,
            parent_task_id,
            task_sub.args.clone(),
            workspace,
            effective_context_id,
        ) {
            Ok((ctx_task_id, resolved_ctx_task_ids)) => {
                let target_id_str = format_contextual_task_id(&ctx_task_id);

                if is_long_lived {
                    if let Some(ref tid) = task_id {
                        long_lived_registry.register(tid.clone(), target_id_str.clone());
                    }
                }

                task_ids.push(target_id_str);
                total_dependency_count += resolved_ctx_task_ids.len().saturating_sub(1);
            }
            Err(e) => {
                return PushTasksResult {
                    task_ids: vec![],
                    dependency_count: 0,
                    attached_long_lived: vec![],
                    error: Some(e.to_string()),
                };
            }
        }
    }

    PushTasksResult {
        task_ids,
        dependency_count: total_dependency_count,
        attached_long_lived,
        error: None,
    }
}

/// Build a TaskId for push_tasks command
fn build_task_id_for_push(task_name: &str, workspace: Option<&str>, project: &Project) -> Option<TaskId> {
    let task_name = TaskName::new(task_name).ok()?;

    let workspace = if let Some(ws_name) = workspace {
        let ident = Ident::new(ws_name);
        project.workspace_by_ident(&ident).ok()?.name.clone()
    } else {
        project.active_workspace().ok()?.name.clone()
    };

    Some(TaskId { workspace, task_name })
}

/// Check if a task is long-lived
fn check_if_long_lived(project: &Project, task_id: &TaskId) -> Option<bool> {
    let workspace = project.workspace_by_ident(&task_id.workspace).ok()?;
    let task_file_path = workspace.taskfile_path();
    let content = task_file_path.fs_read_text().ok()?;
    let task_file = zpm_tasks::parse(&content).ok()?;
    let task = task_file.tasks.get(task_id.task_name.as_str())?;

    Some(task.attributes.iter().any(|attr| attr.name == "long-lived"))
}

// ============================================================================
// Existing Functions
// ============================================================================

fn parse_base_task_id(contextual_task_id: &str) -> Option<TaskId> {
    let (task_part, _context_id)
        = contextual_task_id.rsplit_once('@')?;

    let (workspace_str, task_name_str)
        = task_part.split_once(':')?;

    let task_name
        = TaskName::new(task_name_str).ok()?;

    let workspace
        = Ident::new(workspace_str);

    Some(TaskId {
        workspace,
        task_name,
    })
}

/// Starts the daemon inline (in the current process) and returns the port via a channel.
/// This is used for standalone mode where we want to run the daemon in the same process.
pub async fn start_daemon_inline(project: Arc<Project>, port_tx: tokio::sync::oneshot::Sender<u16>) -> Result<(), Error> {
    run_daemon_internal(project, Some(port_tx)).await
}

pub async fn run_daemon(project: Arc<Project>) -> Result<(), Error> {
    run_daemon_internal(project, None).await
}

async fn run_daemon_internal(project: Arc<Project>, port_tx: Option<tokio::sync::oneshot::Sender<u16>>) -> Result<(), Error> {
    let (listener, port)
        = bind_to_available_port().await?;

    let daemon_url_str
        = daemon_url(port);

    // If a port sender is provided, send the port through it; otherwise print to stdout
    if let Some(tx) = port_tx {
        let _ = tx.send(port);
    } else {
        println!("{}", port);
        let _ = std::io::stdout().flush();
    }

    // Get daemon configuration values
    let output_buffer_max_lines
        = project.config.settings.daemon_output_buffer_max_lines.value;

    let max_closed_tasks
        = project.config.settings.daemon_max_closed_tasks.value;

    let scheduler
        = Arc::new(Scheduler::new());

    let output_buffer: OutputBuffer
        = Arc::new(RwLock::new(HashMap::new()));

    let subscription_registry
        = Arc::new(SubscriptionRegistry::new());

    let long_lived_registry
        = Arc::new(LongLivedRegistry::new());

    let process_registry
        = Arc::new(ProcessRegistry::new());

    // Command channel for handler-to-coordinator communication
    let (command_tx, mut command_rx)
        = mpsc::unbounded_channel::<CoordinatorCommand>();

    let scheduler_for_loop
        = scheduler.clone();

    let process_registry_for_loop
        = process_registry.clone();

    let process_registry_for_signal
        = process_registry.clone();

    // Clone command_tx for use in executor pool
    let command_tx_for_executor
        = command_tx.clone();

    let long_lived_registry_for_loop
        = long_lived_registry.clone();

    let project_for_loop
        = project.clone();

    let (loop_event_tx, mut loop_event_rx)
        = mpsc::unbounded_channel::<ExecutorEvent>();

    // Channel to notify the main loop when warm-up completes
    let (warmup_tx, mut warmup_rx)
        = mpsc::unbounded_channel::<()>();

    let subscription_registry_for_loop
        = subscription_registry.clone();

    let subscription_registry_for_events
        = subscription_registry.clone();

    let output_buffer_for_events
        = output_buffer.clone();

    let long_lived_registry_for_events
        = long_lived_registry.clone();

    let scheduler_for_events
        = scheduler.clone();

    let warmup_tx_for_events
        = warmup_tx.clone();

    // Track closed task IDs in order for cleanup
    let closed_tasks: Arc<RwLock<Vec<String>>>
        = Arc::new(RwLock::new(Vec::new()));

    let closed_tasks_for_events
        = closed_tasks.clone();

    tokio::spawn(async move {
        while let Some(event) = loop_event_rx.recv().await {
            if let ExecutorEvent::Output { task_id, line, stream } = &event {
                match output_buffer_for_events.write() {
                    Ok(mut buffer) => {
                        let lines: &mut Vec<BufferedOutputLine>
                            = buffer
                                .entry(task_id.to_string())
                                .or_insert_with(Vec::new);

                        lines.push(BufferedOutputLine {
                            line: line.to_string(),
                            stream: stream.as_str().to_string(),
                        });

                        if lines.len() > output_buffer_max_lines {
                            let excess
                                = lines.len() - output_buffer_max_lines;

                            lines.drain(0..excess);
                        }
                    }
                    Err(e) => {
                        eprintln!("Failed to acquire output buffer lock: {}", e);
                    }
                }
            }

            // Track task completion for output buffer cleanup
            if let ExecutorEvent::Finished { task_id, .. } | ExecutorEvent::Failed { task_id, .. } = &event {
                if let (Ok(mut closed), Ok(mut buffer)) = (closed_tasks_for_events.write(), output_buffer_for_events.write()) {
                    closed.push(task_id.clone());

                    // Clean up oldest closed task buffers if we exceed the limit
                    while closed.len() > max_closed_tasks {
                        if let Some(oldest_task_id) = closed.first().cloned() {
                            buffer.remove(&oldest_task_id);
                            closed.remove(0);
                        } else {
                            break;
                        }
                    }
                }
            }

            let notification
                = match &event {
                    ExecutorEvent::Started { task_id } => {
                        Some(DaemonNotification::TaskStarted {
                            task_id: task_id.clone(),
                        })
                    }
                    ExecutorEvent::Output { task_id, line, stream } => {
                        Some(DaemonNotification::TaskOutputLine {
                            task_id: task_id.clone(),
                            line: line.clone(),
                            stream: stream.as_str().to_string(),
                        })
                    }
                    ExecutorEvent::Finished { .. } => None,
                    ExecutorEvent::Failed { task_id, error } => {
                        Some(DaemonNotification::TaskFailed {
                            task_id: task_id.clone(),
                            error: error.clone(),
                        })
                    }
                };

            if let Some(n) = notification {
                subscription_registry_for_events.broadcast(n.clone());

                if let DaemonNotification::TaskStarted { task_id } = &n {
                    if let Some(ctx_task_id) = scheduler_for_events.parse_contextual_task_id(task_id) {
                        if scheduler_for_events.is_long_lived(&ctx_task_id) {
                            let task_id_clone
                                = task_id.clone();

                            let ctx_task_id_clone
                                = ctx_task_id.clone();

                            let registry_clone
                                = long_lived_registry_for_events.clone();

                            let sub_registry_clone
                                = subscription_registry_for_events.clone();

                            let scheduler_clone
                                = scheduler_for_events.clone();

                            let warmup_tx_clone
                                = warmup_tx_for_events.clone();

                            tokio::spawn(async move {
                                tokio::time::sleep(Duration::from_millis(LONG_LIVED_WARMUP_MS)).await;

                                // Check if the task has failed or completed during warm-up
                                // If so, skip marking warm-up complete to avoid incorrect state
                                if scheduler_clone.is_failed(&ctx_task_id_clone) || scheduler_clone.is_completed(&ctx_task_id_clone) {
                                    return;
                                }

                                if let Some(base_task_id) = parse_base_task_id(&task_id_clone) {
                                    registry_clone.mark_warm_up_complete(&base_task_id);
                                }

                                scheduler_clone.mark_warm_up_complete(&ctx_task_id_clone);

                                // Notify the main loop to check for newly-ready tasks
                                let _ = warmup_tx_clone.send(());

                                sub_registry_clone.broadcast(DaemonNotification::TaskWarmUpComplete {
                                    task_id: task_id_clone,
                                });
                            });
                        }
                    }
                }
            }
        }
    });

    tokio::spawn(async move {
        let mut executor_pool
            = ExecutorPool::new(loop_event_tx, daemon_url_str, command_tx_for_executor);

        let mut pending_completion: HashMap<ContextualTaskId, i32>
            = HashMap::new();

        // Track tasks currently spawning (between spawn() and PID registration)
        let mut spawning_tasks
            = SpawningTasks::new();

        loop {
            // Process ready tasks and failures
            let running: HashSet<_>
                = executor_pool.running_tasks().cloned().collect();

            let tasks_to_fail
                = scheduler_for_loop.tasks_to_fail(&running);

            for task_id in tasks_to_fail {
                scheduler_for_loop.mark_failed(&task_id);

                let task_id_str
                    = format_contextual_task_id(&task_id);

                subscription_registry_for_loop.broadcast(DaemonNotification::TaskCompleted {
                    task_id: task_id_str,
                    exit_code: 1,
                });
            }

            let ready_tasks
                = scheduler_for_loop.ready_tasks(&running);

            for (task_id, prepared_opt) in ready_tasks {
                // Guard against TOCTOU race: cancel_context() may have marked this task
                // as failed/completed after ready_tasks() returned but before we spawn.
                // Re-check atomically before spawning.
                if !scheduler_for_loop.should_spawn_task(&task_id) {
                    continue;
                }

                if let Some(prepared) = prepared_opt {
                    let task_id_str
                        = format_contextual_task_id(&task_id);

                    // Mark as spawning BEFORE calling spawn() to track the window
                    spawning_tasks.mark_spawning(task_id_str);
                    executor_pool.spawn(task_id, prepared);
                } else {
                    scheduler_for_loop.mark_completed(&task_id);

                    let task_id_str
                        = format_contextual_task_id(&task_id);

                    subscription_registry_for_loop.broadcast(DaemonNotification::TaskCompleted {
                        task_id: task_id_str,
                        exit_code: 0,
                    });
                }
            }

            // Use biased select to prioritize commands over other events
            tokio::select! {
                biased;

                // Handle commands from handlers (highest priority)
                Some(cmd) = command_rx.recv() => {
                    handle_coordinator_command(
                        cmd,
                        &scheduler_for_loop,
                        &process_registry_for_loop,
                        &mut spawning_tasks,
                        &subscription_registry_for_loop,
                        &project_for_loop,
                        &long_lived_registry_for_loop,
                    ).await;
                }

                // Handle task completions from executor
                result = executor_pool.wait_next(), if !executor_pool.is_empty() => {
                    if let Some((task_id, result)) = result {
                        tokio::time::sleep(Duration::from_millis(10)).await;

                        match result {
                            Ok(status) => {
                                let exit_code
                                    = status.code().unwrap_or(-1);

                                scheduler_for_loop.mark_script_finished(&task_id);

                                if !status.success() {
                                    scheduler_for_loop.mark_failed(&task_id);

                                    let task_id_str
                                        = format_contextual_task_id(&task_id);

                                    subscription_registry_for_loop
                                        .broadcast(DaemonNotification::TaskCompleted {
                                            task_id: task_id_str,
                                            exit_code,
                                        });

                                    let parents
                                        = scheduler_for_loop.find_parents(&task_id);

                                    for parent in parents {
                                        if pending_completion.remove(&parent).is_some() {
                                            scheduler_for_loop.mark_failed(&parent);

                                            let parent_id_str
                                                = format_contextual_task_id(&parent);

                                            subscription_registry_for_loop
                                                .broadcast(DaemonNotification::TaskCompleted {
                                                    task_id: parent_id_str,
                                                    exit_code,
                                                });
                                        }
                                    }
                                } else {
                                    if scheduler_for_loop.try_complete_task(&task_id) {
                                        let task_id_str
                                            = format_contextual_task_id(&task_id);

                                        subscription_registry_for_loop
                                            .broadcast(DaemonNotification::TaskCompleted {
                                                task_id: task_id_str,
                                                exit_code,
                                            });

                                        let parents
                                            = scheduler_for_loop.find_parents(&task_id);

                                        for parent in parents {
                                            if let Some(&parent_exit_code) = pending_completion.get(&parent)
                                            {
                                                if scheduler_for_loop.try_complete_task(&parent) {
                                                    pending_completion.remove(&parent);

                                                    let parent_id_str
                                                        = format_contextual_task_id(&parent);

                                                    subscription_registry_for_loop.broadcast(
                                                        DaemonNotification::TaskCompleted {
                                                            task_id: parent_id_str,
                                                            exit_code: parent_exit_code,
                                                        },
                                                    );
                                                }
                                            }
                                        }
                                    } else {
                                        // Check if any subtask has already failed
                                        // This handles the case where the subtask failed before
                                        // the parent script finished
                                        if scheduler_for_loop.has_failed_subtask(&task_id) {
                                            scheduler_for_loop.mark_failed(&task_id);

                                            let task_id_str
                                                = format_contextual_task_id(&task_id);

                                            subscription_registry_for_loop
                                                .broadcast(DaemonNotification::TaskCompleted {
                                                    task_id: task_id_str,
                                                    exit_code: 1,
                                                });
                                        } else {
                                            pending_completion.insert(task_id, exit_code);
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                eprintln!("Task execution error: {}", e);
                            }
                        }
                    }
                }

                // Handle warm-up notifications
                _ = warmup_rx.recv() => {
                    // Warm-up completed; loop will re-check ready tasks
                }

                // Idle polling when no tasks are running
                _ = tokio::time::sleep(Duration::from_millis(50)) => {
                    // Periodic wake-up to check for ready tasks
                }
            }
        }
    });

    let project_root
        = project.project_cwd.clone();

    let initial_inode
        = project_root.fs_metadata()?.ino();

    tokio::spawn(async move {
        watch_project_root(project_root, initial_inode).await;
    });

    // Signal handler for graceful shutdown
    tokio::spawn(async move {
        wait_for_shutdown_signal(process_registry_for_signal).await;
    });

    let ctx
        = Arc::new(ConnectionContext {
        project,
        scheduler,
        subscription_registry,
        output_buffer,
        long_lived_registry,
        process_registry,
        command_tx,
    });

    run_accept_loop(listener, ctx).await;

    Ok(())
}

async fn watch_project_root(project_root: Path, initial_inode: u64) {
    loop {
        tokio::time::sleep(Duration::from_secs(5)).await;

        let current_inode
            = project_root.fs_metadata().map(|m| m.ino()).ok();

        if current_inode != Some(initial_inode) {
            std::process::exit(0);
        }
    }
}

#[cfg(unix)]
async fn wait_for_shutdown_signal(process_registry: Arc<ProcessRegistry>) {
    use tokio::signal::unix::{signal, SignalKind};

    let sigterm = match signal(SignalKind::terminate()) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to register SIGTERM handler: {}", e);
            return;
        }
    };

    let sigint = match signal(SignalKind::interrupt()) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to register SIGINT handler: {}", e);
            return;
        }
    };

    tokio::pin!(sigterm);
    tokio::pin!(sigint);

    tokio::select! {
        _ = sigterm.recv() => {}
        _ = sigint.recv() => {}
    }

    graceful_shutdown(process_registry).await;
}

#[cfg(not(unix))]
async fn wait_for_shutdown_signal(process_registry: Arc<ProcessRegistry>) {
    use tokio::signal::ctrl_c;

    let _ = ctrl_c().await;
    graceful_shutdown(process_registry).await;
}

async fn graceful_shutdown(process_registry: Arc<ProcessRegistry>) {
    let pids = process_registry.get_all_pids();

    if pids.is_empty() {
        std::process::exit(0);
    }

    // First, send SIGTERM to all child process groups for graceful shutdown
    // Since we spawn children with process_group(0), each child is in its own group
    // where the group ID equals the child's PID
    #[cfg(unix)]
    {
        for &pid in &pids {
            // Use killpg to kill the entire process group (negative pid means process group)
            let result = unsafe { libc::killpg(pid as i32, libc::SIGTERM) };
            if result != 0 {
                // If killpg fails (e.g., group doesn't exist), try killing the process directly
                let result = unsafe { libc::kill(pid as i32, libc::SIGTERM) };
                if result != 0 && std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH) {
                    eprintln!("Failed to send SIGTERM to process {}: {}", pid, std::io::Error::last_os_error());
                }
            }
        }
    }

    // Wait 5 seconds for processes to terminate gracefully
    tokio::time::sleep(Duration::from_secs(5)).await;

    // Check which processes are still running and send SIGKILL
    let remaining_pids = process_registry.get_all_pids();

    #[cfg(unix)]
    {
        for pid in remaining_pids {
            // Check if process is still alive before sending SIGKILL
            let alive = unsafe { libc::kill(pid as i32, 0) == 0 };
            if alive {
                // Use killpg to kill the entire process group
                let result = unsafe { libc::killpg(pid as i32, libc::SIGKILL) };
                if result != 0 {
                    // If killpg fails, try killing the process directly
                    let result = unsafe { libc::kill(pid as i32, libc::SIGKILL) };
                    if result != 0 && std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH) {
                        eprintln!("Failed to send SIGKILL to process {}: {}", pid, std::io::Error::last_os_error());
                    }
                }
            }
        }
    }

    std::process::exit(0);
}
