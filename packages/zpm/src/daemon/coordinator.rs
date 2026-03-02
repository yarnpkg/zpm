use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::os::unix::fs::MetadataExt;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use tokio::sync::mpsc;
use super::ipc::{daemon_url, BufferedOutputLine, DaemonNotification};
use zpm_utils::Path;

use super::events::ExecutorEvent;
use super::executor::ExecutorPool;
use super::scheduler::{format_contextual_task_id, ContextualTaskId, Scheduler};
use super::server::{bind_to_available_port, run_accept_loop, ConnectionContext, OutputBuffer};
use super::subscriptions::SubscriptionRegistry;
use crate::error::Error;
use crate::project::Project;

pub async fn run_daemon(project: Arc<Project>) -> Result<(), Error> {
    // Bind to port first so we can pass the daemon URL to the executor
    let (listener, port) = bind_to_available_port().await?;
    let daemon_url_str = daemon_url(port);

    // Print port and flush immediately - important when stdout is piped
    println!("{}", port);
    let _ = std::io::stdout().flush();

    let scheduler = Arc::new(Scheduler::new());
    let output_buffer: OutputBuffer = Arc::new(RwLock::new(HashMap::new()));

    let subscription_registry = Arc::new(SubscriptionRegistry::new());
    let scheduler_for_loop = scheduler.clone();

    let (loop_event_tx, mut loop_event_rx) = mpsc::unbounded_channel::<ExecutorEvent>();

    let subscription_registry_for_loop = subscription_registry.clone();

    // Spawn a separate task to handle output events
    // This ensures output is broadcast immediately as it arrives, not blocked by wait_next()
    let subscription_registry_for_events = subscription_registry.clone();
    let output_buffer_for_events = output_buffer.clone();
    tokio::spawn(async move {
        while let Some(event) = loop_event_rx.recv().await {
            // Store output in buffer
            if let ExecutorEvent::Output { task_id, line, stream } = &event {
                if let Ok(mut buffer) = output_buffer_for_events.write() {
                    let lines: &mut Vec<BufferedOutputLine> = buffer
                        .entry(task_id.to_string())
                        .or_insert_with(Vec::new);
                    lines.push(BufferedOutputLine {
                        line: line.to_string(),
                        stream: stream.as_str().to_string(),
                    });
                }
            }

            let notification = match &event {
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
                // Don't send TaskCompleted here - it's handled in the main loop
                ExecutorEvent::Finished { .. } => None,
                ExecutorEvent::Failed { task_id, error } => {
                    Some(DaemonNotification::TaskFailed {
                        task_id: task_id.clone(),
                        error: error.clone(),
                    })
                }
            };
            if let Some(n) = notification {
                subscription_registry_for_events.broadcast(n);
            }
        }
    });

    // Main coordinator loop
    tokio::spawn(async move {
        let mut executor_pool = ExecutorPool::new(loop_event_tx, daemon_url_str);
        let mut pending_completion: HashMap<ContextualTaskId, i32> = HashMap::new();

        loop {
            let running: HashSet<_> = executor_pool.running_tasks().cloned().collect();

            let tasks_to_fail = scheduler_for_loop.tasks_to_fail(&running);
            for task_id in tasks_to_fail {
                scheduler_for_loop.mark_failed(&task_id);
                let task_id_str = format_contextual_task_id(&task_id);
                subscription_registry_for_loop.broadcast(DaemonNotification::TaskCompleted {
                    task_id: task_id_str,
                    exit_code: 1,
                });
            }

            let ready_tasks = scheduler_for_loop.ready_tasks(&running);
            for (task_id, prepared_opt) in ready_tasks {
                if let Some(prepared) = prepared_opt {
                    executor_pool.spawn(task_id, prepared);
                } else {
                    // No script - immediately mark as completed
                    scheduler_for_loop.mark_completed(&task_id);
                    let task_id_str = format_contextual_task_id(&task_id);
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

            if let Some((task_id, result)) = executor_pool.wait_next().await {
                // Give the output streaming task a moment to flush remaining output
                tokio::time::sleep(Duration::from_millis(10)).await;

                match result {
                    Ok(status) => {
                        let exit_code = status.code().unwrap_or(-1);
                        scheduler_for_loop.mark_script_finished(&task_id);

                        if !status.success() {
                            scheduler_for_loop.mark_failed(&task_id);

                            // Send completion notification for the failed task
                            let task_id_str = format_contextual_task_id(&task_id);
                            subscription_registry_for_loop
                                .broadcast(DaemonNotification::TaskCompleted {
                                    task_id: task_id_str,
                                    exit_code,
                                });

                            // Also fail any parents waiting for this task
                            let parents = scheduler_for_loop.find_parents(&task_id);
                            for parent in parents {
                                if pending_completion.remove(&parent).is_some() {
                                    scheduler_for_loop.mark_failed(&parent);
                                    let parent_id_str = format_contextual_task_id(&parent);
                                    subscription_registry_for_loop
                                        .broadcast(DaemonNotification::TaskCompleted {
                                            task_id: parent_id_str,
                                            exit_code,
                                        });
                                }
                            }
                        } else {
                            if scheduler_for_loop.try_complete_task(&task_id) {
                                // Task is fully completed (no pending subtasks)
                                let task_id_str = format_contextual_task_id(&task_id);
                                subscription_registry_for_loop
                                    .broadcast(DaemonNotification::TaskCompleted {
                                        task_id: task_id_str,
                                        exit_code,
                                    });

                                // Check if any parents can now complete
                                let parents = scheduler_for_loop.find_parents(&task_id);
                                for parent in parents {
                                    if let Some(&parent_exit_code) = pending_completion.get(&parent)
                                    {
                                        if scheduler_for_loop.try_complete_task(&parent) {
                                            pending_completion.remove(&parent);
                                            let parent_id_str = format_contextual_task_id(&parent);
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
                                // Task has pending subtasks - don't send completion yet
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
    });

    let project_root = project.project_cwd.clone();
    let initial_inode = project_root.fs_metadata()?.ino();

    tokio::spawn(async move {
        watch_project_root(project_root, initial_inode).await;
    });

    let ctx = Arc::new(ConnectionContext {
        project,
        scheduler,
        subscription_registry,
        output_buffer,
    });

    run_accept_loop(listener, ctx).await;

    Ok(())
}

async fn watch_project_root(project_root: Path, initial_inode: u64) {
    loop {
        tokio::time::sleep(Duration::from_secs(5)).await;

        let current_inode = project_root.fs_metadata().map(|m| m.ino()).ok();

        if current_inode != Some(initial_inode) {
            std::process::exit(0);
        }
    }
}
