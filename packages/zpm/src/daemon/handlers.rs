use std::sync::Arc;

use tokio::sync::oneshot;
use zpm_utils::ToFileString;

use crate::project::Project;

use super::{
    coordinator_commands::{CommandSender, CoordinatorCommand},
    coordinator_state::SubscriptionId,
    ipc::{DaemonRequest, DaemonResponse, LongLivedTaskStatus, SubscriptionScope},
};

async fn send_command<T>(
    command_tx: &CommandSender,
    build_command: impl FnOnce(oneshot::Sender<T>) -> CoordinatorCommand,
) -> Result<T, DaemonResponse> {
    let (response_tx, response_rx) = oneshot::channel();

    command_tx
        .send(build_command(response_tx))
        .map_err(|_| DaemonResponse::Error {
            message: "Coordinator channel closed".to_string(),
        })?;

    response_rx.await.map_err(|_| DaemonResponse::Error {
        message: "Coordinator did not respond".to_string(),
    })
}

/// Dispatch a daemon request using only the command channel.
/// NO direct access to scheduler, output_buffer, or any other mutable state.
pub async fn dispatch_request(
    request: DaemonRequest,
    subscription_id: Option<SubscriptionId>,
    command_tx: &CommandSender,
    project: &Project,
    port: u16,
    auth_token: Option<&str>,
    shutdown_notify: &Arc<tokio::sync::Notify>,
) -> DaemonResponse {
    match request {
        DaemonRequest::Ping => DaemonResponse::Pong,
        DaemonRequest::GetMeta => DaemonResponse::Meta {
            version: zpm_switch::get_bin_version(),
            cwd: project.project_cwd.to_file_string(),
        },

        DaemonRequest::PushTasks {
            tasks,
            parent_task_id,
            workspace,
            output_subscription: _,
            status_subscription: _,
            context_id,
        } => {
            handle_push_tasks(tasks, parent_task_id, workspace, context_id, subscription_id, command_tx).await
        }

        DaemonRequest::GetTaskOutput { task_id } => {
            handle_get_task_output(task_id, command_tx).await
        }

        DaemonRequest::StopTask { task_name, workspace } => {
            handle_stop_task(task_name, workspace, command_tx).await
        }

        DaemonRequest::ListLongLivedTasks => {
            handle_list_long_lived_tasks(command_tx).await
        }

        DaemonRequest::CancelContext { context_id } => {
            handle_cancel_context(context_id, command_tx).await
        }

        DaemonRequest::ListDeclaredTasks => {
            handle_list_declared_tasks(command_tx).await
        }

        DaemonRequest::GetStats => {
            handle_get_stats(command_tx).await
        }

        DaemonRequest::GetTaskHistory => {
            handle_get_task_history(command_tx).await
        }

        DaemonRequest::GetAuthUrl => {
            handle_get_auth_url(port, auth_token)
        }

        DaemonRequest::Shutdown => {
            handle_shutdown(command_tx, shutdown_notify).await
        }
    }
}

async fn handle_push_tasks(
    tasks: Vec<super::ipc::TaskSubscription>,
    parent_task_id: Option<String>,
    workspace: Option<String>,
    context_id: Option<String>,
    subscription_id: Option<SubscriptionId>,
    command_tx: &CommandSender,
) -> DaemonResponse {
    let (response_tx, response_rx) = oneshot::channel();

    if command_tx
        .send(CoordinatorCommand::PushTasks {
            tasks,
            parent_task_id,
            workspace,
            context_id,
            subscription_id,
            response_tx,
        })
        .is_err()
    {
        return DaemonResponse::Error {
            message: "Coordinator channel closed".to_string(),
        };
    }

    match response_rx.await {
        Ok(result) => {
            if let Some(error) = result.error {
                DaemonResponse::Error { message: error }
            } else {
                DaemonResponse::TasksEnqueued {
                    task_ids: result.task_ids,
                    dependency_count: result.dependency_ids.len(),
                    attached_long_lived: result.attached_long_lived,
                }
            }
        }
        Err(_) => DaemonResponse::Error {
            message: "Coordinator did not respond".to_string(),
        },
    }
}

async fn handle_get_task_output(task_id: super::scheduler::ContextualTaskId, command_tx: &CommandSender) -> DaemonResponse {
    match send_command(command_tx, |response_tx| {
        CoordinatorCommand::GetTaskOutput {
            task_id: task_id.clone(),
            response_tx,
        }
    }).await {
        Ok(lines) => DaemonResponse::TaskOutput { task_id, lines },
        Err(e) => e,
    }
}

async fn handle_stop_task(
    task_name: String,
    workspace: Option<String>,
    command_tx: &CommandSender,
) -> DaemonResponse {
    match send_command(command_tx, |response_tx| {
        CoordinatorCommand::StopTask {
            task_name,
            workspace,
            response_tx,
        }
    }).await {
        Ok(result) => DaemonResponse::TaskStopped {
            success: result.success,
            error: result.error,
        },
        Err(e) => e,
    }
}

async fn handle_list_long_lived_tasks(
    command_tx: &CommandSender,
) -> DaemonResponse {
    match send_command(command_tx, |response_tx| {
        CoordinatorCommand::ListLongLivedTasks { response_tx }
    }).await {
        Ok(entries) => {
            let tasks: Vec<super::ipc::LongLivedTaskInfo> = entries
                .into_iter()
                .map(|info| {
                    let status = LongLivedTaskStatus::Running {
                        started_at_ms: info.started_at_ms,
                        process_id: info.process_id,
                    };

                    super::ipc::LongLivedTaskInfo {
                        workspace: info.task_id.workspace.to_file_string(),
                        task_name: info.task_id.task_name.to_file_string(),
                        status,
                    }
                })
                .collect();

            DaemonResponse::LongLivedTaskList { tasks }
        }
        Err(e) => e,
    }
}

async fn handle_cancel_context(context_id: String, command_tx: &CommandSender) -> DaemonResponse {
    match send_command(command_tx, |response_tx| {
        CoordinatorCommand::CancelContext {
            context_id,
            response_tx,
        }
    }).await {
        Ok(result) => DaemonResponse::ContextCancelled {
            cancelled_count: result.cancelled_count,
        },
        Err(e) => e,
    }
}

async fn handle_get_stats(command_tx: &CommandSender) -> DaemonResponse {
    match send_command(command_tx, |response_tx| {
        CoordinatorCommand::GetStats { response_tx }
    }).await {
        Ok(result) => DaemonResponse::Stats {
            tasks_count: result.tasks_count,
            prepared_count: result.prepared_count,
            subtasks_count: result.subtasks_count,
            output_buffer_count: result.output_buffer_count,
            closed_tasks_count: result.closed_tasks_count,
            watched_files_count: result.watched_files_count,
        },
        Err(e) => e,
    }
}

async fn handle_get_task_history(command_tx: &CommandSender) -> DaemonResponse {
    match send_command(command_tx, |response_tx| {
        CoordinatorCommand::GetTaskHistory { response_tx }
    }).await {
        Ok(events) => DaemonResponse::TaskHistory { events },
        Err(e) => e,
    }
}

async fn handle_shutdown(command_tx: &CommandSender, shutdown_notify: &Arc<tokio::sync::Notify>) -> DaemonResponse {
    let (response_tx, response_rx) = oneshot::channel();

    if command_tx
        .send(CoordinatorCommand::Shutdown { response_tx })
        .is_err()
    {
        shutdown_notify.notify_one();
        return DaemonResponse::Error {
            message: "Coordinator channel closed".to_string(),
        };
    }

    let _ = response_rx.await;
    shutdown_notify.notify_one();

    DaemonResponse::ShuttingDown
}

fn handle_get_auth_url(port: u16, auth_token: Option<&str>) -> DaemonResponse {
    let url = match auth_token {
        Some(token) => {
            let encoded: String = url::form_urlencoded::Serializer::new(String::new())
                .append_pair("token", token)
                .finish();
            format!("http://127.0.0.1:{}/?{}", port, encoded)
        }
        None => format!("http://127.0.0.1:{}/", port),
    };
    DaemonResponse::AuthUrl { url }
}

async fn handle_list_declared_tasks(command_tx: &CommandSender) -> DaemonResponse {
    match send_command(command_tx, |response_tx| {
        CoordinatorCommand::ListDeclaredTasks { response_tx }
    }).await {
        Ok((tasks, errors)) => DaemonResponse::DeclaredTaskList { tasks, errors },
        Err(e) => e,
    }
}

pub async fn create_subscription_if_needed(
    output_scope: SubscriptionScope,
    status_scope: SubscriptionScope,
    context_id: Option<String>,
    command_tx: &CommandSender,
) -> Option<(SubscriptionId, tokio::sync::mpsc::UnboundedReceiver<super::ipc::DaemonNotification>)> {
    if output_scope == SubscriptionScope::None && status_scope == SubscriptionScope::None {
        return None;
    }

    let (response_tx, response_rx) = oneshot::channel();

    if command_tx
        .send(CoordinatorCommand::CreateSubscription {
            output_scope,
            status_scope,
            context_id,
            response_tx,
        })
        .is_err()
    {
        return None;
    }

    response_rx.await.ok()
}
