use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::os::unix::fs::MetadataExt;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use tokio::sync::mpsc;
use super::ipc::{daemon_url, BufferedOutputLine, DaemonNotification};
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

    let scheduler_for_loop
        = scheduler.clone();

    let process_registry_for_executor
        = process_registry.clone();

    let process_registry_for_signal
        = process_registry.clone();

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
            = ExecutorPool::new(loop_event_tx, daemon_url_str, process_registry_for_executor);

        let mut pending_completion: HashMap<ContextualTaskId, i32>
            = HashMap::new();

        loop {
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

            if executor_pool.is_empty() {
                tokio::time::sleep(Duration::from_millis(50)).await;
                continue;
            }

            // Use select! to wait for either:
            // - A task completion from the executor
            // - A warm-up notification (to re-check ready tasks)
            tokio::select! {
                result = executor_pool.wait_next() => {
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
                                        pending_completion.insert(task_id, exit_code);
                                    }
                                }
                            }
                            Err(e) => {
                                eprintln!("Task execution error: {}", e);
                            }
                        }
                    }
                }
                _ = warmup_rx.recv() => {
                    // Warm-up completed; loop will re-check ready tasks
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
