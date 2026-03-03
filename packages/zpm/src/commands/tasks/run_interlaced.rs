use std::{collections::HashSet, io::Write, os::unix::process::ExitStatusExt, process::ExitStatus};

use clipanion::cli;
use uuid::Uuid;
use zpm_utils::ToFileString;

use super::helpers::{format_task_id, format_timestamp, is_long_lived_task, print_attach_header, print_detach_footer};
use crate::daemon::{DaemonClient, DaemonNotification, StandaloneDaemonHandle, SubscriptionScope, TaskSubscription};
use crate::error::Error;
use crate::project::Project;

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

        for attached in &result.attached_long_lived {
            print_attach_header(attached);
        }

        let target_task_ids: HashSet<_>
            = result.task_ids.into_iter()
                .collect();

        let has_long_lived_target
            = target_task_ids.iter().any(|id| is_long_lived_task(id));

        let mut completed_tasks
            = HashSet::new();

        let mut exit_code
            = 0;

        let mut is_first_line
            = true;

        loop {
            let notification
                = tokio::select! {
                    biased;

                    _ = tokio::signal::ctrl_c() => {
                        if has_long_lived_target {
                            // The first line will add a line break to the "^C" message; the second is the true empty line separator
                            println!();

                            if !result.attached_long_lived.is_empty() {
                                println!();
                            }

                            print_detach_footer(&self.name);
                            client.close();
                            if self.standalone {
                                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                            }
                            return Ok(ExitStatus::from_raw(0));
                        } else {
                            continue;
                        }
                    }
                    n = client.recv_notification() => n?,
                };

            match notification {
                DaemonNotification::TaskOutputLine { task_id, line, .. } => {
                    let mut stdout
                        = std::io::stdout().lock();

                    if is_first_line {
                        if !result.attached_long_lived.is_empty() {
                            writeln!(stdout, "").ok();
                        }

                        is_first_line = false;
                    }

                    if self.timestamps {
                        if self.verbose_level >= 1 {
                            writeln!(stdout, "[{}] [{}]: {}", format_timestamp(), format_task_id(&task_id), line).ok();
                        } else {
                            writeln!(stdout, "[{}] {}", format_timestamp(), line).ok();
                        }
                    } else if self.verbose_level >= 1 {
                        writeln!(stdout, "[{}]: {}", format_task_id(&task_id), line).ok();
                    } else {
                        writeln!(stdout, "{}", line).ok();
                    }
                },

                DaemonNotification::TaskStarted { task_id } => {
                    if self.verbose_level >= 2 {
                        let mut stdout
                            = std::io::stdout().lock();

                        if self.timestamps {
                            writeln!(stdout, "[{}] [{}]: Process started", format_timestamp(), format_task_id(&task_id)).ok();
                        } else {
                            writeln!(stdout, "[{}]: Process started", format_task_id(&task_id)).ok();
                        }
                    }
                },

                DaemonNotification::TaskCompleted { task_id, exit_code: code } => {
                    if self.verbose_level >= 2 {
                        let mut stdout
                            = std::io::stdout().lock();

                        if self.timestamps {
                            writeln!(stdout, "[{}] [{}]: Process exited (exit code {})", format_timestamp(), format_task_id(&task_id), code).ok();
                        } else {
                            writeln!(stdout, "[{}]: Process exited (exit code {})", format_task_id(&task_id), code).ok();
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
                        return Err(Error::IpcError(format!("Task {} failed: {}", format_task_id(&task_id), error)));
                    }
                },

                DaemonNotification::TaskWarmUpComplete { .. } => {},
            }
        }

        client.close();
        if self.standalone {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }

        Ok(ExitStatus::from_raw(exit_code << 8))
    }
}
