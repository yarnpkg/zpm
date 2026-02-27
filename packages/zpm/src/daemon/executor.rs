use std::collections::{HashMap, HashSet};
use std::process::ExitStatus;
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use zpm_tasks::TaskId;
use zpm_switch::{DaemonNotification, TASK_CURRENT_ENV};
use zpm_utils::ToFileString;

use crate::daemon::{DynamicExecutionState, PreparedTask};
use crate::error::Error;
use crate::project::Project;
use crate::script::ScriptEnvironment;

/// Notification sender for streaming task output to clients
pub type NotificationSender = mpsc::UnboundedSender<DaemonNotification>;

/// Run the task execution loop
pub async fn run_execution_loop(
    _project: Arc<Project>,
    state: Arc<DynamicExecutionState>,
    notification_tx: NotificationSender,
) {
    let mut running_handles: HashMap<TaskId, JoinHandle<Result<(TaskId, ExitStatus), Error>>> =
        HashMap::new();

    // Track tasks that have finished their script but may have subtasks
    let mut pending_completion: HashMap<TaskId, i32> = HashMap::new();

    loop {
        // Check for ready tasks
        let ready_tasks: Vec<TaskId> = {
            let resolved = state.resolved.read().unwrap();
            let completed = state.completed.read().unwrap();
            let script_finished = state.script_finished.read().unwrap();
            let running: HashSet<TaskId> = running_handles.keys().cloned().collect();

            resolved
                .tasks
                .iter()
                .filter(|(task_id, prerequisites)| {
                    !completed.contains(*task_id)
                        && !script_finished.contains(*task_id)
                        && !running.contains(*task_id)
                        && prerequisites.iter().all(|p| completed.contains(p))
                })
                .map(|(task_id, _)| task_id.clone())
                .collect()
        };

        // Spawn ready tasks
        for task_id in ready_tasks {
            let prepared_opt = {
                let prepared_tasks = state.prepared_tasks.read().unwrap();
                prepared_tasks.get(&task_id).cloned()
            };

            if let Some(prepared) = prepared_opt {
                let task_id_clone = task_id.clone();
                let task_id_str = format!(
                    "{}:{}",
                    task_id.workspace.to_file_string(),
                    task_id.task_name.as_str()
                );

                // Send TaskStarted notification from main loop
                let _ = notification_tx.send(DaemonNotification::TaskStarted {
                    task_id: task_id_str.clone(),
                });

                let tx = notification_tx.clone();

                let handle = tokio::spawn(async move {
                    let result = execute_task(&prepared, &task_id_str, tx.clone()).await;
                    result.map(|status| (task_id_clone, status))
                });

                running_handles.insert(task_id, handle);
            } else {
                // Task has no script, mark as completed immediately
                let mut completed = state.completed.write().unwrap();
                completed.insert(task_id.clone());

                // Send TaskCompleted for no-script tasks
                let task_id_str = format!(
                    "{}:{}",
                    task_id.workspace.to_file_string(),
                    task_id.task_name.as_str()
                );
                let _ = notification_tx.send(DaemonNotification::TaskCompleted {
                    task_id: task_id_str,
                    exit_code: 0,
                });
            }
        }

        // Check pending completions - tasks whose scripts finished but may have subtasks
        let mut newly_completed = Vec::new();
        for (task_id, exit_code) in pending_completion.iter() {
            if state.try_complete_task(task_id) {
                newly_completed.push((task_id.clone(), *exit_code));
            }
        }

        for (task_id, exit_code) in newly_completed {
            pending_completion.remove(&task_id);

            let task_id_str = format!(
                "{}:{}",
                task_id.workspace.to_file_string(),
                task_id.task_name.as_str()
            );
            let _ = notification_tx.send(DaemonNotification::TaskCompleted {
                task_id: task_id_str,
                exit_code,
            });

            // Check if this completion allows parent tasks to complete
            let parents_to_check: Vec<TaskId> = {
                let subtasks = state.subtasks.read().unwrap();
                subtasks
                    .iter()
                    .filter(|(_, children)| children.contains(&task_id))
                    .map(|(parent, _)| parent.clone())
                    .collect()
            };

            for parent in parents_to_check {
                if let Some(&parent_exit_code) = pending_completion.get(&parent) {
                    if state.try_complete_task(&parent) {
                        pending_completion.remove(&parent);

                        let parent_id_str = format!(
                            "{}:{}",
                            parent.workspace.to_file_string(),
                            parent.task_name.as_str()
                        );
                        let _ = notification_tx.send(DaemonNotification::TaskCompleted {
                            task_id: parent_id_str,
                            exit_code: parent_exit_code,
                        });
                    }
                }
            }
        }

        // If no running tasks, wait a bit before checking again
        if running_handles.is_empty() {
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
            continue;
        }

        // Wait for any task to complete
        use futures::future::select_all;
        let handles: Vec<_> = running_handles.drain().collect();
        let task_ids: Vec<_> = handles.iter().map(|(id, _)| id.clone()).collect();
        let futures: Vec<_> = handles
            .into_iter()
            .map(|(_, h)| Box::pin(async move { h.await }))
            .collect();

        let (result, idx, remaining) = select_all(futures).await;

        // Put remaining handles back
        for (future, task_id) in remaining.into_iter().zip(
            task_ids
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != idx)
                .map(|(_, id)| id.clone()),
        ) {
            running_handles.insert(
                task_id,
                tokio::spawn(async move { future.await.unwrap() }),
            );
        }

        // Handle completed task script
        match result {
            Ok(Ok((task_id, status))) => {
                let exit_code = status.code().unwrap_or(-1);

                // Mark script as finished
                {
                    let mut script_finished = state.script_finished.write().unwrap();
                    script_finished.insert(task_id.clone());
                }

                if status.success() {
                    // Try to complete immediately if no subtasks
                    if state.try_complete_task(&task_id) {
                        let task_id_str = format!(
                            "{}:{}",
                            task_id.workspace.to_file_string(),
                            task_id.task_name.as_str()
                        );
                        let _ = notification_tx.send(DaemonNotification::TaskCompleted {
                            task_id: task_id_str,
                            exit_code,
                        });
                    } else {
                        // Has subtasks, add to pending
                        pending_completion.insert(task_id, exit_code);
                    }
                } else {
                    // Task failed - send failure notification
                    let task_id_str = format!(
                        "{}:{}",
                        task_id.workspace.to_file_string(),
                        task_id.task_name.as_str()
                    );
                    let _ = notification_tx.send(DaemonNotification::TaskFailed {
                        task_id: task_id_str,
                        error: format!("Task exited with code {}", exit_code),
                    });

                    // Propagate failure to parent tasks
                    let parents: Vec<TaskId> = {
                        let subtasks = state.subtasks.read().unwrap();
                        subtasks
                            .iter()
                            .filter(|(_, children)| children.contains(&task_id))
                            .map(|(parent, _)| parent.clone())
                            .collect()
                    };

                    for parent in parents {
                        // Remove parent from pending completion
                        pending_completion.remove(&parent);

                        // Send failure notification for parent
                        let parent_id_str = format!(
                            "{}:{}",
                            parent.workspace.to_file_string(),
                            parent.task_name.as_str()
                        );
                        let _ = notification_tx.send(DaemonNotification::TaskFailed {
                            task_id: parent_id_str,
                            error: format!("Subtask failed with code {}", exit_code),
                        });
                    }
                }
            }
            Ok(Err(e)) => {
                // Task execution error
                eprintln!("Task execution error: {}", e);
            }
            Err(e) => {
                // Join error
                eprintln!("Task join error: {}", e);
            }
        }
    }
}

async fn execute_task(
    prepared: &PreparedTask,
    task_id_str: &str,
    notification_tx: NotificationSender,
) -> Result<ExitStatus, Error> {
    let mut env = ScriptEnvironment::new()?;

    for (key, value) in &prepared.env {
        env = env.with_env_variable(key, value);
    }

    // Set the current task ID so nested `yarn tasks push` can pass parent_task_id
    env = env.with_env_variable(TASK_CURRENT_ENV, task_id_str);

    let mut running = env
        .with_cwd(prepared.cwd.clone())
        .spawn_script(&prepared.script, std::iter::empty::<String>())
        .await?;

    let child_stdout = running
        .child
        .stdout
        .take()
        .expect("Failed to capture stdout");
    let child_stderr = running
        .child
        .stderr
        .take()
        .expect("Failed to capture stderr");

    let mut stdout_reader = BufReader::new(child_stdout).lines();
    let mut stderr_reader = BufReader::new(child_stderr).lines();

    let task_id = task_id_str.to_string();

    // Read stdout and stderr concurrently
    loop {
        tokio::select! {
            line = stdout_reader.next_line() => {
                match line {
                    Ok(Some(line)) => {
                        let _ = notification_tx.send(DaemonNotification::TaskOutput {
                            task_id: task_id.clone(),
                            line,
                            stream: "stdout".to_string(),
                        });
                    }
                    Ok(None) => break,
                    Err(_) => break,
                }
            }
            line = stderr_reader.next_line() => {
                match line {
                    Ok(Some(line)) => {
                        let _ = notification_tx.send(DaemonNotification::TaskOutput {
                            task_id: task_id.clone(),
                            line,
                            stream: "stderr".to_string(),
                        });
                    }
                    Ok(None) => {}
                    Err(_) => {}
                }
            }
        }
    }

    // Drain remaining stderr
    while let Ok(Some(line)) = stderr_reader.next_line().await {
        let _ = notification_tx.send(DaemonNotification::TaskOutput {
            task_id: task_id.clone(),
            line,
            stream: "stderr".to_string(),
        });
    }

    let status = running.child.wait().await?;
    Ok(status)
}
