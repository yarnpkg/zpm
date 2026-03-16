// ============================================================================
// Race-Free Coordinator (v3)
//
// This coordinator owns ALL mutable state directly (no Arc<RwLock>).
// All state mutations happen in a single async task via command processing.
// Race conditions are structurally impossible.
//
// Lifecycle transitions are handled by CoordinatorState methods that return
// TransitionEffects. The coordinator loop is a thin dispatcher that applies
// effects (broadcasts notifications, kills PIDs).
// ============================================================================

use std::collections::HashSet;
use std::io::Write;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use tokio::sync::{mpsc, oneshot};
use zpm_primitives::Ident;
use zpm_tasks::{TaskId, TaskName};
use zpm_utils::{Path, ToFileString};

use super::coordinator_commands::{
    CancelContextResult, CommandSender, CoordinatorCommand, LongLivedTaskInfo,
    PushTasksResult, StatsResult, StopTaskResult, TaskCompletionResult,
};
use super::coordinator_state::{
    format_contextual_task_id,
    CoordinatorState, TaskGraph, TransitionEffects,
};
use super::executor::ExecutorPool;
use super::ipc::{daemon_url, AttachedLongLivedTask, BufferedOutputLine, DaemonNotification, TaskSubscription, LONG_LIVED_CONTEXT_ID};
use super::platform;
use super::scheduler::dependencies;
use super::server::{bind_to_available_port, connection::{run_accept_loop, ConnectionContext}};
use crate::error::Error;
use crate::project::Project;

const LONG_LIVED_WARMUP_MS: u64 = 500;

// ============================================================================
// Main Entry Points
// ============================================================================

pub async fn start_daemon_inline(project: Arc<Project>, port_tx: oneshot::Sender<u16>) -> Result<(), Error> {
    run_daemon_internal(project, Some(port_tx)).await
}

pub async fn run_daemon(project: Arc<Project>) -> Result<(), Error> {
    run_daemon_internal(project, None).await
}

async fn run_daemon_internal(
    project: Arc<Project>,
    port_tx: Option<oneshot::Sender<u16>>,
) -> Result<(), Error> {
    let (listener, port) = bind_to_available_port().await?;
    let daemon_url_str = daemon_url(port);

    // Send port through channel or print to stdout
    if let Some(tx) = port_tx {
        let _ = tx.send(port);
    } else {
        println!("{}", port);
        let _ = std::io::stdout().flush();
    }

    // Get configuration
    let output_buffer_max_lines = project.config.settings.daemon_output_buffer_max_lines.value;
    let max_closed_tasks = project.config.settings.daemon_max_closed_tasks.value;

    // Create the command channel
    let (command_tx, command_rx) = mpsc::unbounded_channel::<CoordinatorCommand>();

    // Spawn the coordinator loop
    let project_for_loop = project.clone();
    let command_tx_for_executor = command_tx.clone();

    tokio::spawn(async move {
        run_coordinator_loop(
            project_for_loop,
            command_rx,
            command_tx_for_executor,
            daemon_url_str,
            output_buffer_max_lines,
            max_closed_tasks,
        ).await;
    });

    // Project root watcher
    let project_root = project.project_cwd.clone();
    let command_tx_for_watcher = command_tx.clone();

    tokio::spawn(async move {
        watch_project_root(project_root, command_tx_for_watcher).await;
    });

    // Signal handler
    let command_tx_for_signal = command_tx.clone();
    tokio::spawn(async move {
        wait_for_shutdown_signal(command_tx_for_signal).await;
    });

    // Create simplified connection context (no Arc<RwLock> for mutable state)
    let ctx = Arc::new(ConnectionContext {
        project,
        command_tx,
    });

    // Run accept loop with simplified context
    run_accept_loop(listener, ctx).await;

    Ok(())
}

// ============================================================================
// Coordinator Loop
// ============================================================================

async fn run_coordinator_loop(
    project: Arc<Project>,
    mut command_rx: mpsc::UnboundedReceiver<CoordinatorCommand>,
    command_tx: CommandSender,
    daemon_url: String,
    output_buffer_max_lines: usize,
    max_closed_tasks: usize,
) {
    // Create unified state - owned by this task, no locks
    let mut state = CoordinatorState::new(output_buffer_max_lines, max_closed_tasks);

    // Create executor pool
    let mut executor_pool = ExecutorPool::new(daemon_url, command_tx.clone());

    while let Some(cmd) = command_rx.recv().await {
        let should_shutdown = handle_command(
            cmd,
            &mut state,
            &mut executor_pool,
            &project,
            &command_tx,
        ).await;

        if should_shutdown {
            break;
        }

        process_ready_tasks(&mut state, &mut executor_pool);
    }
}

// ============================================================================
// Apply Effects (thin I/O layer)
// ============================================================================

/// Broadcast notifications and kill PIDs from transition effects.
fn apply_effects(effects: TransitionEffects, subscriptions: &super::coordinator_state::SubscriptionManager) {
    for notification in effects.notifications {
        subscriptions.broadcast(notification);
    }
    for pid in effects.pids_to_kill {
        platform::kill_process_group(pid);
    }
}

// ============================================================================
// Command Handler
// ============================================================================

/// Handle a single command. Returns true if coordinator should shut down.
async fn handle_command(
    cmd: CoordinatorCommand,
    state: &mut CoordinatorState,
    executor_pool: &mut ExecutorPool,
    project: &Project,
    command_tx: &CommandSender,
) -> bool {
    match cmd {
        // ====================================================================
        // Task Management
        // ====================================================================

        CoordinatorCommand::PushTasks {
            tasks,
            parent_task_id,
            workspace,
            context_id,
            subscription_id,
            response_tx,
        } => {
            let result = execute_push_tasks(
                &tasks,
                parent_task_id.as_deref(),
                workspace.as_deref(),
                context_id.as_deref(),
                state,
                project,
            );

            // Add tasks to subscription BEFORE sending the response to avoid race
            // where TaskStarted is processed before the subscription filter is set
            if let Some(sub_id) = subscription_id {
                state.subscriptions.add_tasks(
                    sub_id,
                    result.task_ids.clone(),
                    result.dependency_ids.clone(),
                );
            }

            let _ = response_tx.send(result);
        }

        CoordinatorCommand::CancelContext { context_id, response_tx } => {
            let effects = state.cancel_context(&context_id);
            let cancelled_count = effects.notifications.len();
            apply_effects(effects, &state.subscriptions);

            // Mark spawning tasks for deferred kill (already done inside cancel_context)
            let spawning_ids = state.processes.get_spawning_for_context(&context_id);

            let _ = response_tx.send(CancelContextResult {
                cancelled_count: cancelled_count + spawning_ids.len(),
            });
        }

        CoordinatorCommand::StopTask { task_name, workspace, response_tx } => {
            let result = handle_stop_task(&task_name, workspace.as_deref(), state, project);
            let _ = response_tx.send(result);
        }

        // ====================================================================
        // Process Management
        // ====================================================================

        CoordinatorCommand::RegisterPid { task_id, pid } => {
            // Check if cancelled while spawning
            if let Some(pending_cancel) = state.processes.take_spawning(&task_id) {
                if pending_cancel {
                    platform::kill_process_group(pid);
                    return false;
                }
            }
            state.processes.register_pid(pid, task_id);
        }

        CoordinatorCommand::UnregisterPid { task_id, pid } => {
            state.processes.take_spawning(&task_id);
            state.processes.unregister_pid(pid, &task_id);
        }

        // ====================================================================
        // Executor Events
        // ====================================================================

        CoordinatorCommand::TaskStarted { task_id } => {
            // Broadcast notification
            let task_id_str = format_contextual_task_id(&task_id);
            state.subscriptions.broadcast(DaemonNotification::TaskStarted {
                task_id: task_id_str,
            });

            // Spawn delayed warm-up command for long-lived tasks
            if state.graph.is_long_lived(&task_id) {
                let base_task_id = task_id.task_id.clone();
                let tx = command_tx.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(LONG_LIVED_WARMUP_MS)).await;
                    let _ = tx.send(CoordinatorCommand::WarmUpComplete {
                        task_id,
                        base_task_id,
                    });
                });
            }
        }

        CoordinatorCommand::TaskOutput { task_id, line, stream } => {
            let task_id_str = format_contextual_task_id(&task_id);

            // Buffer output
            state.output.append(task_id_str.clone(), BufferedOutputLine {
                line: line.clone(),
                stream: stream.as_str().to_string(),
            });

            // Broadcast notification
            state.subscriptions.broadcast(DaemonNotification::TaskOutputLine {
                task_id: task_id_str,
                line,
                stream: stream.as_str().to_string(),
            });
        }

        CoordinatorCommand::TaskCompleted { task_id, result } => {
            // Remove from executor's running set BEFORE updating state
            executor_pool.mark_completed(&task_id);

            let exit_code = match result {
                TaskCompletionResult::Exited(status) => status.code().unwrap_or(-1),
                TaskCompletionResult::Error(e) => {
                    eprintln!("Task execution error: {}", e);
                    1
                }
            };

            let effects = state.task_script_finished(&task_id, exit_code);
            apply_effects(effects, &state.subscriptions);
        }

        CoordinatorCommand::WarmUpComplete { task_id, base_task_id } => {
            let effects = state.warm_up_complete(&task_id, &base_task_id);
            apply_effects(effects, &state.subscriptions);
        }

        // ====================================================================
        // Query Commands
        // ====================================================================

        CoordinatorCommand::GetTaskOutput { task_id, response_tx } => {
            let lines = state.output.get(&task_id);
            let _ = response_tx.send(lines);
        }

        CoordinatorCommand::ListLongLivedTasks { response_tx } => {
            let entries = state.long_lived.list();
            let infos: Vec<LongLivedTaskInfo> = entries
                .into_iter()
                .map(|e| {
                    let process_id = state.processes.get_pid_for_task(&e.contextual_task_id);
                    LongLivedTaskInfo {
                        task_id: format!("{}:{}", e.task_id.workspace.to_file_string(), e.task_id.task_name.as_str()),
                        contextual_task_id: format_contextual_task_id(&e.contextual_task_id),
                        warm_up_complete: e.warm_up_complete,
                        started_at_ms: e.started_at
                            .duration_since(SystemTime::UNIX_EPOCH)
                            .map(|d| d.as_millis() as u64)
                            .unwrap_or(0),
                        process_id,
                    }
                })
                .collect();
            let _ = response_tx.send(infos);
        }

        CoordinatorCommand::GetStats { response_tx } => {
            let _ = response_tx.send(StatsResult {
                tasks_count: state.graph.tasks_count(),
                prepared_count: state.graph.prepared_count(),
                subtasks_count: state.graph.subtasks_count(),
                output_buffer_count: state.output.buffer_count(),
                closed_tasks_count: state.output.closed_tasks_count(),
            });
        }

        // ====================================================================
        // Subscription Commands
        // ====================================================================

        CoordinatorCommand::CreateSubscription {
            output_scope,
            status_scope,
            context_id,
            response_tx,
        } => {
            let (id, rx) = state.subscriptions.create(output_scope, status_scope, context_id);
            let _ = response_tx.send((id, rx));
        }

        CoordinatorCommand::AddTasksToSubscription {
            subscription_id,
            target_task_ids,
            dependency_task_ids,
        } => {
            state.subscriptions.add_tasks(subscription_id, target_task_ids, dependency_task_ids);
        }

        CoordinatorCommand::RemoveSubscription { subscription_id } => {
            state.subscriptions.remove(subscription_id);
        }

        // ====================================================================
        // Shutdown
        // ====================================================================

        CoordinatorCommand::Shutdown { response_tx } => {
            let pids = state.processes.get_all_pids();
            let _ = response_tx.send(pids);
            return true; // Signal shutdown
        }
    }

    false
}

// ============================================================================
// Ready Task Processing
// ============================================================================

fn process_ready_tasks(state: &mut CoordinatorState, executor_pool: &mut ExecutorPool) {
    let running: HashSet<_> = executor_pool.running_tasks().cloned().collect();

    // Find tasks to cancel (dependencies failed)
    let tasks_to_cancel = dependencies::find_tasks_to_fail(&state.graph, &running);
    for task_id in tasks_to_cancel {
        let effects = state.cancel_task(&task_id);
        apply_effects(effects, &state.subscriptions);
    }

    // Find ready tasks
    let ready_ids = dependencies::find_ready_tasks(&state.graph, &running);
    let ready_tasks: Vec<_> = ready_ids
        .into_iter()
        .map(|ctx_task_id| {
            let prepared = state.graph.prepared.get(&ctx_task_id).cloned();
            (ctx_task_id, prepared)
        })
        .collect();

    for (task_id, prepared_opt) in ready_tasks {
        // Atomic check - no race possible, we own the state
        if !state.graph.should_spawn_task(&task_id) {
            continue;
        }

        if let Some(prepared) = prepared_opt {
            // Mark as spawning BEFORE spawn
            state.processes.mark_spawning(task_id.clone());

            // Spawn task
            executor_pool.spawn(task_id, prepared);
        } else {
            // No script - complete immediately
            let effects = state.complete_no_script(&task_id);
            apply_effects(effects, &state.subscriptions);
        }
    }
}

// ============================================================================
// Push Tasks Handler
// ============================================================================

fn execute_push_tasks(
    tasks: &[TaskSubscription],
    parent_task_id: Option<&str>,
    workspace: Option<&str>,
    context_id: Option<&str>,
    state: &mut CoordinatorState,
    project: &Project,
) -> PushTasksResult {
    let mut task_ids = Vec::new();
    let mut dependency_ids = Vec::new();
    let mut attached_long_lived = Vec::new();

    for task_sub in tasks {
        let task_id = build_task_id(&task_sub.name, workspace, project);

        // Check if this is a long-lived task. Use filesystem fallback on first
        // push when the graph cache hasn't been populated yet.
        let is_long_lived = task_id
            .as_ref()
            .map(|tid| resolve_is_long_lived(&state.graph, project, tid))
            .unwrap_or(false);

        // For long-lived tasks, check if already running
        if is_long_lived {
            if let Some(ref tid) = task_id {
                if let Some(existing) = state.long_lived.get(tid) {
                    let existing_id_str = format_contextual_task_id(&existing.contextual_task_id);
                    task_ids.push(existing_id_str.clone());

                    let started_at_ms = existing
                        .started_at
                        .duration_since(SystemTime::UNIX_EPOCH)
                        .map(|d| d.as_millis() as u64)
                        .unwrap_or(0);

                    attached_long_lived.push(AttachedLongLivedTask {
                        task_id: existing_id_str,
                        started_at_ms,
                    });

                    continue;
                }
            }
        }

        let effective_context_id = if is_long_lived {
            Some(LONG_LIVED_CONTEXT_ID)
        } else {
            context_id
        };

        match state.graph.add_task(
            project,
            &task_sub.name,
            parent_task_id,
            task_sub.args.clone(),
            workspace,
            effective_context_id,
        ) {
            Ok((ctx_task_id, resolved_ctx_task_ids)) => {
                let target_id_str = format_contextual_task_id(&ctx_task_id);

                // After add_task, check is_long_lived from the prepared task
                // (which was populated by prepare_specific_tasks without extra I/O)
                let is_long_lived = state.graph.is_long_lived(&ctx_task_id) || is_long_lived;

                if is_long_lived {
                    // Register in long-lived registry
                    if let Some(ref tid) = task_id {
                        state.long_lived.register(tid.clone(), ctx_task_id.clone());
                    }
                }

                task_ids.push(target_id_str.clone());

                for resolved_id in &resolved_ctx_task_ids {
                    let resolved_str = format_contextual_task_id(resolved_id);
                    if resolved_str != target_id_str {
                        dependency_ids.push(resolved_str);
                    }
                }
            }
            Err(e) => {
                return PushTasksResult {
                    task_ids: vec![],
                    dependency_ids: vec![],
                    attached_long_lived: vec![],
                    error: Some(e.to_string()),
                };
            }
        }
    }

    PushTasksResult {
        task_ids,
        dependency_ids,
        attached_long_lived,
        error: None,
    }
}

// ============================================================================
// Stop Task Handler
// ============================================================================

fn handle_stop_task(
    task_name: &str,
    workspace: Option<&str>,
    state: &mut CoordinatorState,
    project: &Project,
) -> StopTaskResult {
    let task_id = match build_task_id(task_name, workspace, project) {
        Some(tid) => tid,
        None => {
            return StopTaskResult {
                success: false,
                error: Some(format!("Could not resolve task: {}", task_name)),
            };
        }
    };

    let contextual_task_id = match state.long_lived.get(&task_id) {
        Some(e) => e.contextual_task_id.clone(),
        None => {
            return StopTaskResult {
                success: false,
                error: Some(format!("No running long-lived task found: {}", task_name)),
            };
        }
    };

    let effects = state.stop_long_lived(&task_id, &contextual_task_id);
    apply_effects(effects, &state.subscriptions);

    StopTaskResult {
        success: true,
        error: None,
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

fn build_task_id(task_name: &str, workspace: Option<&str>, project: &Project) -> Option<TaskId> {
    let task_name = TaskName::new(task_name).ok()?;

    let workspace = if let Some(ws_name) = workspace {
        let ident = Ident::new(ws_name);
        project.workspace_by_ident(&ident).ok()?.name.clone()
    } else {
        project.active_workspace().ok()?.name.clone()
    };

    Some(TaskId { workspace, task_name })
}

/// Check if a task is long-lived, with filesystem fallback for first push.
fn resolve_is_long_lived(graph: &TaskGraph, project: &Project, task_id: &TaskId) -> bool {
    // Fast path: check graph cache (populated after first add_task)
    if check_if_long_lived_from_graph(graph, task_id) {
        return true;
    }

    // Slow path: resolve from disk (only needed on first push per task)
    project.resolve_task(task_id)
        .ok()
        .and_then(|resolved| {
            resolved.task_files.get(&task_id.workspace)
                .and_then(|tf| tf.tasks.get(task_id.task_name.as_str()))
                .map(|task| task.attributes.iter().any(|a| a.name == "long-lived"))
        })
        .unwrap_or(false)
}

/// Check if a task is long-lived by examining already-resolved task files in the graph.
fn check_if_long_lived_from_graph(graph: &TaskGraph, task_id: &TaskId) -> bool {
    if let Some(task_file) = graph.resolved.task_files.get(&task_id.workspace) {
        if let Some(task) = task_file.tasks.get(task_id.task_name.as_str()) {
            return task.attributes.iter().any(|attr| attr.name == "long-lived");
        }
    }
    false
}

// ============================================================================
// Watchers and Signal Handlers
// ============================================================================

async fn watch_project_root(project_root: Path, command_tx: CommandSender) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let initial_inode = match project_root.fs_metadata().map(|m| m.ino()) {
            Ok(ino) => ino,
            Err(_) => return,
        };

        loop {
            tokio::time::sleep(Duration::from_secs(5)).await;

            let current_inode = project_root.fs_metadata().map(|m| m.ino()).ok();

            if current_inode != Some(initial_inode) {
                graceful_shutdown(command_tx).await;
                return;
            }
        }
    }

    #[cfg(not(unix))]
    {
        // On non-Unix platforms, just keep the watcher alive without inode checking
        let _ = command_tx;
        let _ = project_root;
        loop {
            tokio::time::sleep(Duration::from_secs(60)).await;
        }
    }
}

#[cfg(unix)]
async fn wait_for_shutdown_signal(command_tx: CommandSender) {
    use tokio::signal::unix::{signal, SignalKind};

    let sigterm = signal(SignalKind::terminate()).ok();
    let sigint = signal(SignalKind::interrupt()).ok();

    match (sigterm, sigint) {
        (Some(mut term), Some(mut int)) => {
            tokio::select! {
                _ = term.recv() => {}
                _ = int.recv() => {}
            }
        }
        _ => {
            tokio::signal::ctrl_c().await.ok();
        }
    }

    graceful_shutdown(command_tx).await;
}

#[cfg(not(unix))]
async fn wait_for_shutdown_signal(command_tx: CommandSender) {
    let _ = tokio::signal::ctrl_c().await;
    graceful_shutdown(command_tx).await;
}

async fn graceful_shutdown(command_tx: CommandSender) {
    let (response_tx, response_rx) = oneshot::channel();

    if command_tx.send(CoordinatorCommand::Shutdown { response_tx }).is_err() {
        std::process::exit(0);
    }

    let pids = response_rx.await.unwrap_or_default();

    if pids.is_empty() {
        std::process::exit(0);
    }

    // Send SIGTERM
    for &pid in &pids {
        platform::kill_process_group(pid);
    }

    // Wait 5 seconds
    tokio::time::sleep(Duration::from_secs(5)).await;

    // Force kill remaining
    for &pid in &pids {
        if platform::is_process_alive(pid) {
            platform::kill_process(pid);
        }
    }

    std::process::exit(0);
}
