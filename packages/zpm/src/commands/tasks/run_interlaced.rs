use std::{collections::HashSet, io::Write, os::unix::process::ExitStatusExt, process::ExitStatus};

use chrono::Local;
use clipanion::cli;
use uuid::Uuid;
use zpm_utils::ToFileString;

use crate::daemon::{DaemonClient, DaemonNotification, StandaloneDaemonHandle, SubscriptionScope, TaskSubscription};
use crate::error::Error;
use crate::project::Project;

fn display_task_id(task_id: &str) -> &str {
    task_id.rsplit_once('@').map(|(base, _)| base).unwrap_or(task_id)
}

fn current_timestamp() -> String {
    Local::now().format("%Y-%m-%dT%H:%M:%S%.3f").to_string()
}

#[cli::command(proxy)]
#[cli::path("tasks", "run")]
#[cli::category("Scripting commands")]
pub struct TaskRunInterlaced {
    #[cli::option("-v,--verbose", default = if zpm_utils::is_terminal() {2} else {0}, counter)]
    verbose_level: u8,

    #[cli::option("--timestamps", default = false)]
    timestamps: bool,

    #[cli::option("--standalone", default = false)]
    standalone: bool,

    name: String,
    args: Vec<String>,
}

impl TaskRunInterlaced {
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
                    SubscriptionScope::FullTree,
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

        let mut completed_tasks
            = HashSet::new();

        let mut exit_code
            = 0;

        loop {
            let notification
                = client.recv_notification().await?;

            match notification {
                DaemonNotification::TaskOutputLine { task_id, line, .. } => {
                    let mut stdout
                        = std::io::stdout().lock();

                    if self.timestamps {
                        if self.verbose_level >= 1 {
                            writeln!(stdout, "[{}] [{}]: {}", current_timestamp(), display_task_id(&task_id), line).ok();
                        } else {
                            writeln!(stdout, "[{}] {}", current_timestamp(), line).ok();
                        }
                    } else if self.verbose_level >= 1 {
                        writeln!(stdout, "[{}]: {}", display_task_id(&task_id), line).ok();
                    } else {
                        writeln!(stdout, "{}", line).ok();
                    }
                },

                DaemonNotification::TaskStarted { task_id } => {
                    if self.verbose_level >= 2 {
                        let mut stdout
                            = std::io::stdout().lock();

                        if self.timestamps {
                            writeln!(stdout, "[{}] [{}]: Process started", current_timestamp(), display_task_id(&task_id)).ok();
                        } else {
                            writeln!(stdout, "[{}]: Process started", display_task_id(&task_id)).ok();
                        }
                    }
                },

                DaemonNotification::TaskCompleted { task_id, exit_code: code } => {
                    if self.verbose_level >= 2 {
                        let mut stdout
                            = std::io::stdout().lock();

                        if self.timestamps {
                            writeln!(stdout, "[{}] [{}]: Process exited (exit code {})", current_timestamp(), display_task_id(&task_id), code).ok();
                        } else {
                            writeln!(stdout, "[{}]: Process exited (exit code {})", display_task_id(&task_id), code).ok();
                        }
                    }

                    if target_task_ids.contains(&task_id) {
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
                    if target_task_ids.contains(&task_id) {
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
