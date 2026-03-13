// ============================================================================
// Race-Free Coordinator (v2)
//
// This coordinator owns ALL mutable state directly (no Arc<RwLock>).
// All state mutations happen in a single async task via command processing.
// Race conditions are structurally impossible.
// ============================================================================

use std::collections::HashSet;
use std::io::Write;
use std::os::unix::fs::MetadataExt;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use tokio::sync::{mpsc, oneshot};
use zpm_primitives::Ident;
use zpm_tasks::{TaskId, TaskName};
use zpm_utils::{Path, ToFileString};

use super::coordinator_commands::{
    CancelContextResult, CommandSender, CoordinatorCommand, LongLivedTaskInfo,
    PushTasksResult, StopTaskResult,
};
use super::coordinator_state::{
    format_contextual_task_id, parse_base_task_id, CoordinatorState, ContextualTaskId, PreparedTask,
};
use super::executor::ExecutorPool;
use super::ipc::{daemon_url, AttachedLongLivedTask, BufferedOutputLine, DaemonNotification, TaskSubscription, LONG_LIVED_CONTEXT_ID};
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
    let initial_inode = project_root.fs_metadata()?.ino();
    let command_tx_for_watcher = command_tx.clone();

    tokio::spawn(async move {
        watch_project_root(project_root, initial_inode, command_tx_for_watcher).await;
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
// Simplified Connection Context
// ============================================================================


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
    let mut executor_pool = ExecutorPool::new(daemon_url, command_tx);

    loop {
        // Process ready tasks and failures FIRST
        process_ready_tasks(&mut state, &mut executor_pool);

        // Process warm-up deadlines
        let warm_up_completed = state.process_warm_up_deadlines();
        for (ctx_task_id, base_task_id) in warm_up_completed {
            state.mark_warm_up_complete(&ctx_task_id);
            state.mark_long_lived_warm_up_complete(&base_task_id);

            let task_id_str = format_contextual_task_id(&ctx_task_id);
            state.broadcast(DaemonNotification::TaskWarmUpComplete {
                task_id: task_id_str,
            });
        }

        // Wait for next event
        tokio::select! {
            biased;

            // Commands have highest priority
            Some(cmd) = command_rx.recv() => {
                let should_shutdown = handle_command(
                    cmd,
                    &mut state,
                    &mut executor_pool,
                    &project,
                ).await;

                if should_shutdown {
                    break;
                }
            }

            // Task completions from executor
            result = executor_pool.wait_next(), if !executor_pool.is_empty() => {
                if let Some((task_id, result)) = result {
                    handle_task_completion(
                        task_id,
                        result,
                        &mut state,
                    );
                }
            }

            // Periodic wake-up for ready task checking and warm-up deadlines
            _ = tokio::time::sleep(Duration::from_millis(50)) => {
                // Loop will re-check ready tasks and warm-up deadlines
            }
        }
    }
}

// ============================================================================
// Command Handler
// ============================================================================

/// Handle a single command. Returns true if coordinator should shut down.
async fn handle_command(
    cmd: CoordinatorCommand,
    state: &mut CoordinatorState,
    _executor_pool: &mut ExecutorPool,
    project: &Project,
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
            let _ = response_tx.send(result);
        }

        CoordinatorCommand::CancelContext { context_id, response_tx } => {
            // 1. Mark tasks as failed in scheduler
            let cancelled_ids = state.cancel_context(&context_id);

            // 2. Get and kill registered PIDs
            let pids = state.take_pids_for_context(&context_id);
            for pid in &pids {
                kill_process_group(*pid);
            }

            // 3. Mark spawning tasks for deferred kill
            let spawning_ids = state.get_spawning_for_context(&context_id);
            for task_id in &spawning_ids {
                state.mark_spawning_pending_cancel(task_id);
            }

            let _ = response_tx.send(CancelContextResult {
                cancelled_count: cancelled_ids.len() + spawning_ids.len(),
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
            if let Some(pending_cancel) = state.take_spawning(&task_id) {
                if pending_cancel {
                    kill_process_group(pid);
                    return false;
                }
            }
            state.register_pid(pid, task_id);
        }

        CoordinatorCommand::UnregisterPid { task_id, pid } => {
            state.take_spawning(&task_id);
            state.unregister_pid(pid, &task_id);
        }

        // ====================================================================
        // Executor Events (integrated into command loop)
        // ====================================================================

        CoordinatorCommand::TaskStarted { task_id } => {
            // Broadcast notification
            state.broadcast(DaemonNotification::TaskStarted {
                task_id: task_id.clone(),
            });

            // Schedule warm-up for long-lived tasks
            if let Some(ctx_task_id) = state.parse_contextual_task_id_simple(&task_id) {
                if state.is_long_lived(&ctx_task_id) {
                    if let Some(base_task_id) = parse_base_task_id(&task_id) {
                        state.schedule_warm_up(
                            ctx_task_id,
                            base_task_id,
                            Duration::from_millis(LONG_LIVED_WARMUP_MS),
                        );
                    }
                }
            }
        }

        CoordinatorCommand::TaskOutput { task_id, line, stream } => {
            // Buffer output
            state.buffer_output(task_id.clone(), BufferedOutputLine {
                line: line.clone(),
                stream: stream.as_str().to_string(),
            });

            // Broadcast notification
            state.broadcast(DaemonNotification::TaskOutputLine {
                task_id,
                line,
                stream: stream.as_str().to_string(),
            });
        }

        CoordinatorCommand::TaskFailed { task_id, error } => {
            state.broadcast(DaemonNotification::TaskFailed {
                task_id: task_id.clone(),
                error,
            });

            if let Some(ctx_task_id) = state.parse_contextual_task_id_simple(&task_id) {
                state.mark_failed(&ctx_task_id);
            }

            state.mark_task_closed(task_id);
        }

        // ====================================================================
        // Query Commands
        // ====================================================================

        CoordinatorCommand::GetTaskOutput { task_id, response_tx } => {
            let lines = state.get_task_output(&task_id);
            let _ = response_tx.send(lines);
        }

        CoordinatorCommand::ListLongLivedTasks { response_tx } => {
            let entries = state.list_long_lived();
            let infos: Vec<LongLivedTaskInfo> = entries
                .into_iter()
                .map(|e| LongLivedTaskInfo {
                    task_id: format!("{}:{}", e.task_id.workspace.to_file_string(), e.task_id.task_name.as_str()),
                    contextual_task_id: e.contextual_task_id,
                    warm_up_complete: e.warm_up_complete,
                    started_at_ms: e.started_at
                        .duration_since(SystemTime::UNIX_EPOCH)
                        .map(|d| d.as_millis() as u64)
                        .unwrap_or(0),
                })
                .collect();
            let _ = response_tx.send(infos);
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
            let (id, rx) = state.create_subscription(output_scope, status_scope, context_id);
            let _ = response_tx.send((id, rx));
        }

        CoordinatorCommand::AddTasksToSubscription {
            subscription_id,
            target_task_ids,
            dependency_task_ids,
        } => {
            state.add_tasks_to_subscription(subscription_id, target_task_ids, dependency_task_ids);
        }

        CoordinatorCommand::RemoveSubscription { subscription_id } => {
            state.remove_subscription(subscription_id);
        }

        // ====================================================================
        // Shutdown
        // ====================================================================

        CoordinatorCommand::Shutdown { response_tx } => {
            let pids = state.get_all_pids();
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
    let tasks_to_cancel = find_tasks_to_fail(state, &running);
    for task_id in tasks_to_cancel {
        state.mark_cancelled(&task_id);

        let task_id_str = format_contextual_task_id(&task_id);
        state.broadcast(DaemonNotification::TaskCancelled {
            task_id: task_id_str,
        });
    }

    // Find ready tasks
    let ready_tasks = find_ready_tasks(state, &running);

    for (task_id, prepared_opt) in ready_tasks {
        // Atomic check - no race possible, we own the state
        if !state.should_spawn_task(&task_id) {
            continue;
        }

        if let Some(prepared) = prepared_opt {
            let task_id_str = format_contextual_task_id(&task_id);

            // Mark as spawning BEFORE spawn
            state.mark_spawning(task_id_str);

            // Spawn task
            executor_pool.spawn(task_id, prepared);
        } else {
            // No script - complete immediately
            state.mark_completed(&task_id);

            let task_id_str = format_contextual_task_id(&task_id);
            state.broadcast(DaemonNotification::TaskCompleted {
                task_id: task_id_str,
                exit_code: 0,
            });
        }
    }
}

fn find_ready_tasks(
    state: &CoordinatorState,
    running: &HashSet<ContextualTaskId>,
) -> Vec<(ContextualTaskId, Option<PreparedTask>)> {
    let ready_ids = dependencies::find_ready_tasks(state, running);

    ready_ids
        .into_iter()
        .map(|ctx_task_id| {
            let prepared = state.prepared.get(&ctx_task_id).cloned();
            (ctx_task_id, prepared)
        })
        .collect()
}

fn find_tasks_to_fail(
    state: &CoordinatorState,
    running: &HashSet<ContextualTaskId>,
) -> Vec<ContextualTaskId> {
    dependencies::find_tasks_to_fail(state, running)
}

// ============================================================================
// Task Completion Handling
// ============================================================================

fn handle_task_completion(
    task_id: ContextualTaskId,
    result: Result<std::process::ExitStatus, Error>,
    state: &mut CoordinatorState,
) {
    match result {
        Ok(status) => {
            let exit_code = status.code().unwrap_or(-1);
            // Mark script finished with exit code (stored in WaitingForSubtasks state)
            state.mark_script_finished_with_code(&task_id, exit_code);

            if !status.success() {
                handle_task_failure(&task_id, exit_code, state);
            } else {
                handle_task_success(&task_id, exit_code, state);
            }
        }
        Err(e) => {
            eprintln!("Task execution error: {}", e);
            handle_task_failure(&task_id, 1, state);
        }
    }
}

fn handle_task_failure(
    task_id: &ContextualTaskId,
    exit_code: i32,
    state: &mut CoordinatorState,
) {
    state.mark_failed(task_id);

    let task_id_str = format_contextual_task_id(task_id);
    state.broadcast(DaemonNotification::TaskCompleted {
        task_id: task_id_str,
        exit_code,
    });

    // Propagate failure to parents that are waiting for subtasks
    let parents = state.find_parents(task_id);
    for parent in parents {
        if state.get_waiting_exit_code(&parent).is_some() {
            state.mark_failed(&parent);

            let parent_id_str = format_contextual_task_id(&parent);
            state.broadcast(DaemonNotification::TaskCompleted {
                task_id: parent_id_str,
                exit_code,
            });
        }
    }
}

fn handle_task_success(
    task_id: &ContextualTaskId,
    exit_code: i32,
    state: &mut CoordinatorState,
) {
    if state.try_complete_task(task_id) {
        let task_id_str = format_contextual_task_id(task_id);
        state.broadcast(DaemonNotification::TaskCompleted {
            task_id: task_id_str,
            exit_code,
        });

        // Try to complete parents that are waiting for subtasks
        let parents = state.find_parents(task_id);
        for parent in parents {
            if let Some(parent_exit_code) = state.get_waiting_exit_code(&parent) {
                if state.try_complete_task(&parent) {
                    let parent_id_str = format_contextual_task_id(&parent);
                    state.broadcast(DaemonNotification::TaskCompleted {
                        task_id: parent_id_str,
                        exit_code: parent_exit_code,
                    });
                }
            }
        }
    } else {
        // Check if any subtask has already failed
        if state.has_failed_subtask(task_id) {
            state.mark_failed(task_id);

            let task_id_str = format_contextual_task_id(task_id);
            state.broadcast(DaemonNotification::TaskCompleted {
                task_id: task_id_str,
                exit_code: 1,
            });
        }
        // Otherwise task stays in WaitingForSubtasks state until all subtasks complete
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
        let task_id = build_task_id_for_push(&task_sub.name, workspace, project);

        let is_long_lived = task_id
            .as_ref()
            .and_then(|tid| check_if_long_lived(project, tid))
            .unwrap_or(false);

        // For long-lived tasks, check if already running
        if is_long_lived {
            if let Some(ref tid) = task_id {
                if let Some(existing) = state.get_long_lived(tid) {
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

        match state.add_task(
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
                        state.register_long_lived(tid.clone(), target_id_str.clone());
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
    let task_id = match build_task_id_for_stop(task_name, workspace, project) {
        Some(tid) => tid,
        None => {
            return StopTaskResult {
                success: false,
                error: Some(format!("Could not resolve task: {}", task_name)),
            };
        }
    };

    let entry = match state.get_long_lived(&task_id) {
        Some(e) => e,
        None => {
            return StopTaskResult {
                success: false,
                error: Some(format!("No running long-lived task found: {}", task_name)),
            };
        }
    };

    // Check if spawning
    if state.mark_spawning_pending_cancel(&entry.contextual_task_id) {
        state.remove_long_lived(&task_id);
        return StopTaskResult {
            success: true,
            error: Some("Task is spawning, will be killed shortly".to_string()),
        };
    }

    // Try to take and kill PID
    if let Some(pid) = state.take_pid_for_task(&entry.contextual_task_id) {
        kill_process_group(pid);
        state.remove_long_lived(&task_id);
        return StopTaskResult {
            success: true,
            error: None,
        };
    }

    // No PID - task may have completed
    state.remove_long_lived(&task_id);
    StopTaskResult {
        success: true,
        error: Some("Task had no process ID, removed from registry".to_string()),
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

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

fn build_task_id_for_stop(task_name: &str, workspace: Option<&str>, project: &Project) -> Option<TaskId> {
    build_task_id_for_push(task_name, workspace, project)
}

fn check_if_long_lived(project: &Project, task_id: &TaskId) -> Option<bool> {
    let workspace = project.workspace_by_ident(&task_id.workspace).ok()?;
    let task_file_path = workspace.taskfile_path();
    let content = task_file_path.fs_read_text().ok()?;
    let task_file = zpm_tasks::parse(&content).ok()?;
    let task = task_file.tasks.get(task_id.task_name.as_str())?;

    Some(task.attributes.iter().any(|attr| attr.name == "long-lived"))
}

#[cfg(unix)]
fn kill_process_group(pid: u32) {
    let result = unsafe { libc::killpg(pid as i32, libc::SIGTERM) };
    if result != 0 {
        let _ = unsafe { libc::kill(pid as i32, libc::SIGTERM) };
    }
}

#[cfg(not(unix))]
fn kill_process_group(_pid: u32) {}

// ============================================================================
// Watchers and Signal Handlers
// ============================================================================

async fn watch_project_root(project_root: Path, initial_inode: u64, command_tx: CommandSender) {
    loop {
        tokio::time::sleep(Duration::from_secs(5)).await;

        let current_inode = project_root.fs_metadata().map(|m| m.ino()).ok();

        if current_inode != Some(initial_inode) {
            graceful_shutdown(command_tx).await;
            return;
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
    #[cfg(unix)]
    {
        for &pid in &pids {
            let result = unsafe { libc::killpg(pid as i32, libc::SIGTERM) };
            if result != 0 {
                let _ = unsafe { libc::kill(pid as i32, libc::SIGTERM) };
            }
        }
    }

    // Wait 5 seconds
    tokio::time::sleep(Duration::from_secs(5)).await;

    // Send SIGKILL to remaining
    #[cfg(unix)]
    {
        for &pid in &pids {
            let alive = unsafe { libc::kill(pid as i32, 0) == 0 };
            if alive {
                let result = unsafe { libc::killpg(pid as i32, libc::SIGKILL) };
                if result != 0 {
                    let _ = unsafe { libc::kill(pid as i32, libc::SIGKILL) };
                }
            }
        }
    }

    std::process::exit(0);
}

