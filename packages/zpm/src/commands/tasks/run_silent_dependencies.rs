use std::sync::Arc;
use std::{collections::HashSet, io::Write, os::unix::process::ExitStatusExt, process::ExitStatus};

use clipanion::{Environment, cli};
use uuid::Uuid;
use zpm_utils::{is_terminal, start_progress, ToFileString};

use crate::daemon::{DaemonClient, DaemonNotification, ProgressState, StandaloneDaemonHandle, SubscriptionScope, TaskSubscription};
use crate::error::Error;
use crate::project::Project;

fn display_task_id(task_id: &str) -> &str {
    task_id.rsplit_once('@').map(|(base, _)| base).unwrap_or(task_id)
}

#[cli::command(proxy)]
#[cli::path("tasks", "run")]
#[cli::category("Scripting commands")]
pub struct TaskRunSilentDependencies {
    #[cli::option("--silent-dependencies")]
    _silent_dependencies: bool,

    #[cli::option("-v,--verbose", default = if zpm_utils::is_terminal() {2} else {0}, counter)]
    verbose_level: u8,

    #[cli::option("--standalone", default = false)]
    standalone: bool,

    name: String,
    args: Vec<String>,
}

impl TaskRunSilentDependencies {
    pub fn new(cli_environment: &Environment, name: String, args: Vec<String>) -> Self {
        Self {
            cli_environment: cli_environment.clone(),
            cli_path: vec!["tasks".to_string(), "run".to_string()],
            _silent_dependencies: true,
            verbose_level: 0,
            standalone: false,
            name,
            args,
        }
    }

    pub async fn execute(&self) -> Result<ExitStatus, Error> {
        let mut project
            = Project::new(None).await?;

        project.lazy_install().await?;

        let workspace
            = project.active_workspace()?;

        let workspace_name
            = workspace.name.to_file_string();

        let _daemon_handle: Option<StandaloneDaemonHandle>;

        let mut client
            = if self.standalone {
                let (c, handle)
                    = DaemonClient::connect_standalone(&project.project_cwd).await?;

                _daemon_handle = Some(handle);
                c
            } else {
                _daemon_handle = None;
                DaemonClient::connect(&project.project_cwd).await?
            };

        let context_id
            = Uuid::new_v4().to_string();

        let task_subscriptions
            = vec![TaskSubscription {
                name: self.name.to_string(),
                args: self.args.to_vec(),
            }];

        let result
            = client
                .push_tasks_with_subscriptions(
                    task_subscriptions,
                    None,
                    Some(workspace_name),
                    SubscriptionScope::TargetOnly,
                    SubscriptionScope::FullTree,
                    Some(context_id),
                )
                .await?;

        if result.task_ids.is_empty() {
            return Err(Error::TaskPushFailed("No tasks enqueued".to_string()));
        }

        let target_task_ids: HashSet<_>
            = result.task_ids.into_iter()
                .collect();

        let show_progress
            = is_terminal() && result.dependency_count > 0;

        let mut progress_handle
            = if show_progress {
                let progress_state
                    = Arc::new(ProgressState::new(result.dependency_count));

                let progress_state_clone
                    = progress_state.clone();

                Some((
                    start_progress(move |frame_idx| progress_state_clone.format_progress(frame_idx)),
                    progress_state,
                ))
            } else {
                None
            };

        let mut completed_tasks
            = HashSet::new();

        let mut exit_code
            = 0;

        loop {
            let notification
                = client.recv_notification().await?;

            match notification {
                DaemonNotification::TaskOutputLine { line, .. } => {
                    let mut stdout
                        = std::io::stdout().lock();

                    writeln!(stdout, "{}", line).ok();
                },

                DaemonNotification::TaskStarted { task_id } => {
                    let is_target
                        = target_task_ids.contains(&task_id);

                    if is_target {
                        if let Some((ref mut handle, _)) = progress_handle {
                            handle.stop();
                        }
                    } else {
                        if let Some((_, ref progress_state)) = progress_handle {
                            progress_state.add_task(display_task_id(&task_id));
                        }
                    }
                },

                DaemonNotification::TaskCompleted { task_id, exit_code: code } => {
                    let is_target
                        = target_task_ids.contains(&task_id);

                    if !is_target {
                        if let Some((_, ref progress_state)) = progress_handle {
                            progress_state.remove_task(display_task_id(&task_id));
                        }

                        if code != 0 {
                            if let Some((ref mut handle, _)) = progress_handle {
                                handle.stop();
                            }

                            let mut stdout
                                = std::io::stdout().lock();

                            writeln!(stdout, "[{}]: Process started", display_task_id(&task_id)).ok();

                            if let Ok(lines) = client.get_task_output(&task_id).await {
                                for output_line in lines {
                                    writeln!(stdout, "[{}]: {}", display_task_id(&task_id), output_line.line).ok();
                                }
                            }

                            writeln!(stdout, "[{}]: Process exited (exit code {})", display_task_id(&task_id), code).ok();
                        }
                    }

                    if is_target {
                        completed_tasks.insert(task_id);
                        if code != 0 {
                            exit_code = code;
                        }
                    }

                    if completed_tasks.len() >= target_task_ids.len() {
                        break;
                    }
                },

                DaemonNotification::TaskFailed { task_id, error } => {
                    let is_target
                        = target_task_ids.contains(&task_id);

                    if let Some((ref mut handle, _)) = progress_handle {
                        handle.stop();
                    }

                    let mut stdout
                        = std::io::stdout().lock();

                    writeln!(stdout, "[{}]: Process started", display_task_id(&task_id)).ok();

                    if let Ok(lines) = client.get_task_output(&task_id).await {
                        for output_line in lines {
                            writeln!(stdout, "[{}]: {}", display_task_id(&task_id), output_line.line).ok();
                        }
                    }

                    if is_target {
                        client.close();
                        if self.standalone {
                            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                        }
                        return Err(Error::IpcError(format!("Task {} failed: {}", display_task_id(&task_id), error)));
                    }
                },
            }
        }

        client.close();
        if self.standalone {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }

        Ok(ExitStatus::from_raw(exit_code << 8))
    }
}
