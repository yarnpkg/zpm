use std::{collections::HashSet, io::Write, sync::Arc, time::{Duration, SystemTime}};

use tokio::sync::{mpsc, oneshot};
use zpm_primitives::Ident;
use zpm_tasks::{TaskId, TaskName};
use zpm_utils::Path;

use super::{
    coordinator_commands::{
        CancelContextResult, CommandSender, CoordinatorCommand, LongLivedTaskInfo,
        PushTasksResult, StatsResult, StopTaskResult, TaskCompletionResult,
    },
    coordinator_state::{
        now_ms, ContextualTaskId, CoordinatorState, TaskGraph, TransitionEffects, LONG_LIVED_ATTRIBUTE,
    },
    executor::ExecutorPool,
    ipc::{daemon_url, AttachedLongLivedTask, BufferedOutputLine, DaemonNotification, TaskEvent, TaskEventState, TaskSubscription, LONG_LIVED_CONTEXT_ID},
    platform,
    scheduler::dependencies,
    server::{bind_to_available_port, connection::{run_accept_loop, ConnectionContext}},
};
use crate::{
    error::Error,
    project::Project,
};

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
    let (listener, port)
        = bind_to_available_port().await?;
    let daemon_url_str
        = daemon_url(port);

    // Send port through channel or print to stdout
    if let Some(tx) = port_tx {
        let _ = tx.send(port);
    } else {
        println!("{}", port);
        let _ = std::io::stdout().flush();
    }

    let output_buffer_max_lines
        = project.config.settings.daemon_output_buffer_max_lines.value;
    let max_closed_tasks
        = project.config.settings.daemon_max_closed_tasks.value;
    let default_warmup_period
        = project.config.settings.daemon_default_warmup_period.value;

    let (command_tx, command_rx)
        = mpsc::unbounded_channel::<CoordinatorCommand>();

    // Shutdown signal: when notified, the accept loop exits cleanly
    let shutdown_notify
        = Arc::new(tokio::sync::Notify::new());

    // Spawn the coordinator loop
    let project_for_loop
        = project.clone();
    let command_tx_for_executor
        = command_tx.clone();

    tokio::spawn(async move {
        run_coordinator_loop(
            project_for_loop,
            command_rx,
            command_tx_for_executor,
            daemon_url_str,
            output_buffer_max_lines,
            max_closed_tasks,
            default_warmup_period,
        ).await;
    });

    // Project root watcher
    let project_root
        = project.project_cwd.clone();
    let command_tx_for_watcher
        = command_tx.clone();
    let shutdown_for_watcher
        = shutdown_notify.clone();

    tokio::spawn(async move {
        watch_project_root(project_root, command_tx_for_watcher, shutdown_for_watcher).await;
    });

    // Signal handler
    let command_tx_for_signal
        = command_tx.clone();
    let shutdown_for_signal
        = shutdown_notify.clone();
    tokio::spawn(async move {
        wait_for_shutdown_signal(command_tx_for_signal, shutdown_for_signal).await;
    });

    let ctx = Arc::new(ConnectionContext {
        command_tx,
    });

    // Run accept loop until shutdown is signaled
    tokio::select! {
        _ = run_accept_loop(listener, ctx) => {}
        _ = shutdown_notify.notified() => {}
    }

    Ok(())
}

async fn run_coordinator_loop(
    project: Arc<Project>,
    mut command_rx: mpsc::UnboundedReceiver<CoordinatorCommand>,
    command_tx: CommandSender,
    daemon_url: String,
    output_buffer_max_lines: usize,
    max_closed_tasks: usize,
    default_warmup_period: Duration,
) {
    let mut state
        = CoordinatorState::new(output_buffer_max_lines, max_closed_tasks);

    let mut executor_pool
        = ExecutorPool::new(daemon_url, command_tx.clone());

    while let Some(cmd) = command_rx.recv().await {
        let should_shutdown = handle_command(
            cmd,
            &mut state,
            &mut executor_pool,
            &project,
            &command_tx,
            default_warmup_period,
        ).await;

        if should_shutdown {
            break;
        }

        process_ready_tasks(&mut state, &mut executor_pool);
    }
}

/// Broadcast notifications and kill PIDs from transition effects.
fn apply_effects(effects: TransitionEffects, subscriptions: &super::coordinator_state::SubscriptionManager) {
    for notification in effects.notifications {
        subscriptions.broadcast(notification);
    }
    for pid in effects.pids_to_kill {
        platform::kill_process_group(pid);
    }
}

/// Record task events from transition effects into the event history.
///
/// Note: TaskStarted and WarmUpComplete (Live) events are recorded directly
/// in their respective command handlers where the PID is readily available.
fn record_events_from_effects(effects: &TransitionEffects, event_history: &mut super::coordinator_state::EventHistory) {
    let date
        = now_ms();

    for notification in &effects.notifications {
        match notification {
            DaemonNotification::TaskCompleted { task_id, exit_code, signal } => {
                event_history.push(TaskEvent {
                    date,
                    contextual_task_id: task_id.clone(),
                    state: if *exit_code == 0 {
                        TaskEventState::Completed
                    } else {
                        TaskEventState::Failed {
                            exit_code: Some(*exit_code),
                            signal: *signal,
                        }
                    },
                });
            }
            DaemonNotification::TaskCancelled { task_id, .. } => {
                event_history.push(TaskEvent {
                    date,
                    contextual_task_id: task_id.clone(),
                    state: TaskEventState::Cancelled,
                });
            }
            _ => {
                // TaskStarted, TaskWarmUpComplete, and TaskOutputLine are
                // either handled elsewhere or are not state changes.
            }
        }
    }
}

/// Record events and apply effects (broadcast + kill) in one step.
fn dispatch_effects(effects: TransitionEffects, state: &mut CoordinatorState) {
    record_events_from_effects(&effects, &mut state.event_history);
    apply_effects(effects, &state.subscriptions);
}

/// Handle a single command. Returns true if coordinator should shut down.
async fn handle_command(
    cmd: CoordinatorCommand,
    state: &mut CoordinatorState,
    executor_pool: &mut ExecutorPool,
    project: &Project,
    command_tx: &CommandSender,
    default_warmup_period: Duration,
) -> bool {
    match cmd {
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
            let effects
                = state.cancel_context(&context_id);
            let cancelled_count
                = effects.notifications.len();

            dispatch_effects(effects, state);

            // Mark spawning tasks for deferred kill (already done inside cancel_context)
            let spawning_ids
                = state.processes.get_spawning_for_context(&context_id);

            let _ = response_tx.send(CancelContextResult {
                cancelled_count: cancelled_count + spawning_ids.len(),
            });
        }

        CoordinatorCommand::StopTask { task_name, workspace, response_tx } => {
            let result
                = handle_stop_task(&task_name, workspace.as_deref(), state, project);
            let _ = response_tx.send(result);
        }

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

        CoordinatorCommand::TaskStarted { task_id, pid } => {
            state.subscriptions.broadcast(DaemonNotification::TaskStarted {
                task_id: task_id.clone(),
            });

            let is_long_lived
                = state.graph.is_long_lived(&task_id);

            // Record event: long-lived tasks enter WarmUp, regular tasks are Started
            // Use pid 0 as fallback when PID is unavailable (shouldn't happen in practice)
            let event_pid
                = pid.unwrap_or(0);

            state.event_history.push(TaskEvent {
                date: now_ms(),
                contextual_task_id: task_id.clone(),
                state: if is_long_lived {
                    TaskEventState::WarmUp { pid: event_pid }
                } else {
                    TaskEventState::Started { pid: event_pid }
                },
            });

            // Spawn delayed warm-up command for long-lived tasks
            if is_long_lived {
                let base_task_id
                    = task_id.task_id.clone();
                let tx
                    = command_tx.clone();

                tokio::spawn(async move {
                    tokio::time::sleep(default_warmup_period).await;
                    let _ = tx.send(CoordinatorCommand::WarmUpComplete {
                        task_id,
                        base_task_id,
                    });
                });
            }
        }

        CoordinatorCommand::TaskOutput { task_id, line, stream } => {
            state.output.append(task_id.clone(), BufferedOutputLine {
                line: line.clone(),
                stream: stream.as_str().to_string(),
            });

            state.subscriptions.broadcast(DaemonNotification::TaskOutputLine {
                task_id,
                line,
                stream: stream.as_str().to_string(),
            });
        }

        CoordinatorCommand::TaskCompleted { task_id, result } => {
            executor_pool.mark_completed(&task_id);

            let (exit_code, signal) = match result {
                TaskCompletionResult::Exited(status) => {
                    let code
                        = status.code().unwrap_or(-1);
                    #[cfg(unix)]
                    let sig = {
                        use std::os::unix::process::ExitStatusExt;
                        status.signal()
                    };
                    #[cfg(not(unix))]
                    let sig = None;
                    (code, sig)
                }
                TaskCompletionResult::Error(e) => {
                    eprintln!("Task execution error: {}", e);
                    (1, None)
                }
            };

            let effects
                = state.task_script_finished(&task_id, exit_code, signal);
            dispatch_effects(effects, state);
        }

        CoordinatorCommand::WarmUpComplete { task_id, base_task_id } => {
            // Look up PID before warm_up_complete (which doesn't modify process registry)
            let pid
                = state.processes.get_pid_for_task(&task_id).unwrap_or(0);

            let effects
                = state.warm_up_complete(&task_id, &base_task_id);

            // Record Live event directly here with PID from process registry
            if !effects.notifications.is_empty() {
                state.event_history.push(TaskEvent {
                    date: now_ms(),
                    contextual_task_id: task_id.clone(),
                    state: TaskEventState::Live { pid },
                });
            }

            apply_effects(effects, &state.subscriptions);
        }

        CoordinatorCommand::GetTaskOutput { task_id, response_tx } => {
            let lines
                = state.output.get(&task_id);
            let _ = response_tx.send(lines);
        }

        CoordinatorCommand::ListLongLivedTasks { response_tx } => {
            let entries
                = state.long_lived.list();

            let infos: Vec<LongLivedTaskInfo> = entries
                .into_iter()
                .map(|e| {
                    let process_id
                        = state.processes.get_pid_for_task(&e.contextual_task_id);

                    LongLivedTaskInfo {
                        task_id: e.task_id.clone(),
                        contextual_task_id: e.contextual_task_id.clone(),
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

        CoordinatorCommand::GetTaskHistory { response_tx } => {
            let _ = response_tx.send(state.event_history.list());
        }

        CoordinatorCommand::CreateSubscription {
            output_scope,
            status_scope,
            context_id,
            response_tx,
        } => {
            let (id, rx)
                = state.subscriptions.create(output_scope, status_scope, context_id);
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

        CoordinatorCommand::Shutdown { response_tx } => {
            let pids
                = state.processes.get_all_pids();
            let _ = response_tx.send(pids);
            return true;
        }
    }

    false
}

fn process_ready_tasks(state: &mut CoordinatorState, executor_pool: &mut ExecutorPool) {
    let running: HashSet<_>
        = executor_pool.running_tasks().cloned().collect();

    // Find tasks to cancel (dependencies failed)
    let tasks_to_cancel
        = dependencies::find_tasks_to_fail(&state.graph, &running);

    for task_id in tasks_to_cancel {
        let effects
            = state.cancel_task(&task_id);
        dispatch_effects(effects, state);
    }

    // Find ready tasks
    let ready_ids
        = dependencies::find_ready_tasks(&state.graph, &running);

    let ready_tasks: Vec<_> = ready_ids
        .into_iter()
        .filter_map(|ctx_task_id| {
            let prepared
                = state.graph.prepared.get(&ctx_task_id)?.clone();
            Some((ctx_task_id, prepared))
        })
        .collect();

    for (task_id, prepared) in ready_tasks {
        // Atomic check - no race possible, we own the state
        if !state.graph.should_spawn_task(&task_id) {
            continue;
        }

        if prepared.script.is_empty() {
            // No script - complete immediately
            let effects
                = state.complete_no_script(&task_id);
            dispatch_effects(effects, state);
        } else {
            // Mark as spawning BEFORE spawn
            state.processes.mark_spawning(task_id.clone());

            // Spawn task
            executor_pool.spawn(task_id, prepared);
        }
    }
}

fn execute_push_tasks(
    tasks: &[TaskSubscription],
    parent_task_id: Option<&str>,
    workspace: Option<&str>,
    context_id: Option<&str>,
    state: &mut CoordinatorState,
    project: &Project,
) -> PushTasksResult {
    let mut task_ids = vec![];
    let mut dependency_ids = vec![];
    let mut attached_long_lived = vec![];

    for task_sub in tasks {
        let task_id
            = build_task_id(&task_sub.name, workspace, project);

        // Check if this is a long-lived task. Use filesystem fallback on first
        // push when the graph cache hasn't been populated yet.
        let is_long_lived = task_id
            .as_ref()
            .map(|tid| resolve_is_long_lived(&state.graph, project, tid))
            .unwrap_or(false);

        // For long-lived tasks, try to attach to an already-running instance
        if is_long_lived {
            if let Some(attached) = try_attach_long_lived(&task_id, state) {
                task_ids.push(attached.task_id.clone());
                attached_long_lived.push(attached);
                continue;
            }
        }

        let effective_context_id = if is_long_lived {
            Some(LONG_LIVED_CONTEXT_ID)
        } else {
            context_id
        };

        // For long-lived task restarts, clear stale graph state so
        // add_task's dedup check allows the re-add.
        if is_long_lived {
            prepare_long_lived_restart(&task_id, state);
        }

        match state.graph.add_task(
            project,
            &task_sub.name,
            parent_task_id,
            task_sub.args.clone(),
            workspace,
            effective_context_id,
            &mut state.contexts,
        ) {
            Ok((ctx_task_id, resolved_ctx_task_ids)) => {
                record_scheduled_events(&resolved_ctx_task_ids, &mut state.event_history);

                // After add_task, check is_long_lived from the prepared task
                // (which was populated by prepare_specific_tasks without extra I/O)
                if is_long_lived || state.graph.is_long_lived(&ctx_task_id) {
                    if let Some(ref tid) = task_id {
                        state.long_lived.register(tid.clone(), ctx_task_id.clone());
                    }
                }

                for resolved_id in &resolved_ctx_task_ids {
                    if *resolved_id != ctx_task_id {
                        dependency_ids.push(resolved_id.clone());
                    }
                }

                task_ids.push(ctx_task_id);
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

/// Try to attach to an already-running long-lived task.
/// Returns the attachment info if the task is currently running.
fn try_attach_long_lived(
    task_id: &Option<TaskId>,
    state: &CoordinatorState,
) -> Option<AttachedLongLivedTask> {
    let tid
        = task_id.as_ref()?;
    let existing
        = state.long_lived.get(tid)?;
    let existing_ctx_id
        = existing.contextual_task_id.clone();

    let started_at_ms = existing
        .started_at
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    Some(AttachedLongLivedTask {
        task_id: existing_ctx_id,
        started_at_ms,
    })
}

/// Clear stale graph state for a long-lived task restart.
fn prepare_long_lived_restart(task_id: &Option<TaskId>, state: &mut CoordinatorState) {
    if let Some(tid) = task_id {
        let ctx_tid = ContextualTaskId::new(
            tid.clone(),
            LONG_LIVED_CONTEXT_ID.to_string(),
        );
        state.graph.clear_task_state(&ctx_tid);
    }
}

/// Record "scheduled" events for all newly resolved tasks.
fn record_scheduled_events(
    resolved_ids: &[ContextualTaskId],
    event_history: &mut super::coordinator_state::EventHistory,
) {
    let date
        = now_ms();

    for resolved_id in resolved_ids {
        event_history.push(TaskEvent {
            date,
            contextual_task_id: resolved_id.clone(),
            state: TaskEventState::Scheduled,
        });
    }
}

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

    let effects
        = state.stop_long_lived(&task_id, &contextual_task_id);
    dispatch_effects(effects, state);

    StopTaskResult {
        success: true,
        error: None,
    }
}

fn build_task_id(task_name: &str, workspace: Option<&str>, project: &Project) -> Option<TaskId> {
    let task_name
        = TaskName::new(task_name).ok()?;

    let workspace = if let Some(ws_name) = workspace {
        let ident
            = Ident::new(ws_name);
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
                .map(|task| task.attributes.iter().any(|a| a.name == LONG_LIVED_ATTRIBUTE))
        })
        .unwrap_or(false)
}

/// Check if a task is long-lived by examining already-resolved task files in the graph.
fn check_if_long_lived_from_graph(graph: &TaskGraph, task_id: &TaskId) -> bool {
    if let Some(task_file) = graph.resolved.task_files.get(&task_id.workspace) {
        if let Some(task) = task_file.tasks.get(task_id.task_name.as_str()) {
            return task.attributes.iter().any(|attr| attr.name == LONG_LIVED_ATTRIBUTE);
        }
    }
    false
}

async fn watch_project_root(project_root: Path, command_tx: CommandSender, shutdown_notify: Arc<tokio::sync::Notify>) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;

        let initial_inode = match project_root.fs_metadata().map(|m| m.ino()) {
            Ok(ino) => ino,
            Err(_) => return,
        };

        loop {
            tokio::time::sleep(Duration::from_secs(5)).await;

            let current_inode
                = project_root.fs_metadata().map(|m| m.ino()).ok();

            if current_inode != Some(initial_inode) {
                graceful_shutdown(command_tx, shutdown_notify).await;
                return;
            }
        }
    }

    #[cfg(not(unix))]
    {
        // On non-Unix platforms, just keep the watcher alive without inode checking
        let _ = command_tx;
        let _ = project_root;
        let _ = shutdown_notify;
        loop {
            tokio::time::sleep(Duration::from_secs(60)).await;
        }
    }
}

#[cfg(unix)]
async fn wait_for_shutdown_signal(command_tx: CommandSender, shutdown_notify: Arc<tokio::sync::Notify>) {
    use tokio::signal::unix::{signal, SignalKind};

    let sigterm
        = signal(SignalKind::terminate()).ok();
    let sigint
        = signal(SignalKind::interrupt()).ok();

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

    graceful_shutdown(command_tx, shutdown_notify).await;
}

#[cfg(not(unix))]
async fn wait_for_shutdown_signal(command_tx: CommandSender, shutdown_notify: Arc<tokio::sync::Notify>) {
    let _ = tokio::signal::ctrl_c().await;
    graceful_shutdown(command_tx, shutdown_notify).await;
}

async fn graceful_shutdown(command_tx: CommandSender, shutdown_notify: Arc<tokio::sync::Notify>) {
    let (response_tx, response_rx)
        = oneshot::channel();

    if command_tx.send(CoordinatorCommand::Shutdown { response_tx }).is_err() {
        shutdown_notify.notify_one();
        return;
    }

    let pids
        = response_rx.await.unwrap_or_default();

    if !pids.is_empty() {
        for &pid in &pids {
            platform::kill_process_group(pid);
        }

        tokio::time::sleep(Duration::from_secs(5)).await;

        for &pid in &pids {
            if platform::is_process_alive(pid) {
                platform::kill_process(pid);
            }
        }
    }

    shutdown_notify.notify_one();
}
