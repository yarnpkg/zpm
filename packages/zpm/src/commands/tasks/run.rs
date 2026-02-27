use std::{collections::HashSet, io::Write, os::unix::process::ExitStatusExt, process::ExitStatus};

use clipanion::cli;
use zpm_switch::{DaemonNotification, SubscriptionKind, TaskSubscription};
use zpm_tasks::{parse, TaskName};

use zpm_utils::ToFileString;

use crate::{daemon::DaemonClient, error::Error, project::Project};

#[cli::command(proxy)]
#[cli::path("tasks", "run")]
#[cli::category("Scripting commands")]
pub struct TaskRun {
    #[cli::option("-i,--interlaced", default = true)]
    interlaced: bool,

    #[cli::option("-v,--verbose", default = if zpm_utils::is_terminal() {2} else {0}, counter)]
    verbose_level: u8,

    #[cli::option("--silent-dependencies", default = false)]
    silent_dependencies: bool,

    name: String,
    args: Vec<String>,
}

impl TaskRun {
    pub async fn execute(&self) -> Result<ExitStatus, Error> {
        let mut project = Project::new(None).await?;
        project.lazy_install().await?;

        run_task_impl(
            &project,
            &self.name,
            &self.args,
            self.verbose_level,
            self.silent_dependencies,
        )
        .await
    }
}

pub async fn run_task(
    project: &Project,
    name: &str,
    _args: &[String],
    verbose_level: u8,
    silent_dependencies: bool,
    _interlaced: bool,
    _enable_timers: bool,
) -> Result<ExitStatus, Error> {
    run_task_impl(project, name, _args, verbose_level, silent_dependencies).await
}

async fn run_task_impl(
    project: &Project,
    name: &str,
    _args: &[String],
    verbose_level: u8,
    silent_dependencies: bool,
) -> Result<ExitStatus, Error> {
    let task_name = TaskName::new(name)
        .map_err(|_| Error::TaskNameParseError(name.to_string()))?;

    let workspace = project.active_workspace()?;

    let task_file_path = workspace.taskfile_path();

    if !task_file_path.fs_exists() {
        return Err(Error::TaskFileNotFound(workspace.path.clone()));
    }

    let task_file_content = task_file_path.fs_read_text()?;
    let task_file = parse(&task_file_content).map_err(Error::TaskParseError)?;

    if !task_file.tasks.contains_key(task_name.as_str()) {
        return Err(Error::TaskNotFound {
            workspace: workspace.name.clone(),
            task_name: name.to_string(),
        });
    }

    // Connect to daemon
    let mut client
        = DaemonClient::connect(&project.project_cwd).await?;

    // Push task with output and status subscriptions
    let task_subscriptions = vec![TaskSubscription {
        name: name.to_string(),
        subscriptions: vec![SubscriptionKind::Output, SubscriptionKind::Status],
        args: _args.to_vec(),
    }];

    // Get the workspace name to pass to daemon
    let workspace_name = workspace.name.to_file_string();
    let task_ids = client.push_tasks(task_subscriptions, None, Some(workspace_name)).await?;

    if task_ids.is_empty() {
        return Err(Error::TaskPushFailed("No tasks enqueued".to_string()));
    }

    let target_task_ids: HashSet<String> = task_ids.into_iter().collect();

    // Listen for notifications until all target tasks complete
    let mut completed_tasks: HashSet<String> = HashSet::new();
    let mut exit_code = 0;

    // For silent_dependencies mode, we buffer dependency output so we can show it on failure
    let mut buffered_output: Vec<String> = Vec::new();
    let mut had_failure = false;

    loop {
        let notification = client.recv_notification().await?;

        match notification {
            DaemonNotification::TaskOutput { task_id, line, stream: _ } => {
                let is_target = target_task_ids.contains(&task_id);

                if silent_dependencies {
                    if is_target {
                        // Output from target task - show without prefix
                        let mut stdout = std::io::stdout().lock();
                        writeln!(stdout, "{}", line).ok();
                    } else {
                        // Output from dependency - buffer it
                        buffered_output.push(format!("[{}]: {}", task_id, line));
                    }
                } else if verbose_level >= 1 {
                    let mut stdout = std::io::stdout().lock();
                    writeln!(stdout, "[{}]: {}", task_id, line).ok();
                } else {
                    let mut stdout = std::io::stdout().lock();
                    writeln!(stdout, "{}", line).ok();
                }
            }
            DaemonNotification::TaskStarted { task_id } => {
                let is_target = target_task_ids.contains(&task_id);

                if silent_dependencies && !is_target {
                    // Buffer the start message
                    buffered_output.push(format!("[{}]: Process started", task_id));
                } else if verbose_level >= 2 {
                    let mut stdout = std::io::stdout().lock();
                    writeln!(stdout, "[{}]: Process started", task_id).ok();
                }
            }
            DaemonNotification::TaskCompleted { task_id, exit_code: code } => {
                let is_target = target_task_ids.contains(&task_id);

                if silent_dependencies && !is_target {
                    // Buffer the completion message
                    buffered_output.push(format!("[{}]: Process exited (exit code {})", task_id, code));
                } else if verbose_level >= 2 {
                    let mut stdout = std::io::stdout().lock();
                    writeln!(stdout, "[{}]: Process exited (exit code {})", task_id, code).ok();
                }

                if is_target {
                    completed_tasks.insert(task_id.clone());
                    if code != 0 {
                        exit_code = code;
                    }
                }

                if completed_tasks.len() >= target_task_ids.len() {
                    break;
                }
            }
            DaemonNotification::TaskFailed { task_id, error } => {
                let is_target = target_task_ids.contains(&task_id);

                if is_target {
                    // On failure, print buffered output if we're in silent_dependencies mode
                    if silent_dependencies && !buffered_output.is_empty() {
                        had_failure = true;
                        let mut stdout = std::io::stdout().lock();
                        for line in &buffered_output {
                            writeln!(stdout, "{}", line).ok();
                        }
                    }

                    return Err(Error::IpcError(format!("Task {} failed: {}", task_id, error)));
                } else if silent_dependencies {
                    // Dependency failed - print buffered output and mark failure
                    had_failure = true;
                    let mut stdout = std::io::stdout().lock();
                    for line in &buffered_output {
                        writeln!(stdout, "{}", line).ok();
                    }
                    buffered_output.clear();
                }
            }
        }
    }

    // If we had a failure (non-zero exit), print buffered output
    if silent_dependencies && exit_code != 0 && !had_failure && !buffered_output.is_empty() {
        let mut stdout = std::io::stdout().lock();
        for line in &buffered_output {
            writeln!(stdout, "{}", line).ok();
        }
    }

    // On Unix, ExitStatus::from_raw expects the raw wait status where exit code is shifted by 8
    Ok(ExitStatus::from_raw(exit_code << 8))
}

pub fn task_exists(project: &Project, task_name: &str) -> bool {
    let Ok(task_name)
        = TaskName::new(task_name)
    else {
        return false;
    };

    let Ok(workspace)
        = project.active_workspace()
    else {
        return false;
    };

    let task_file_path
        = workspace.taskfile_path();

    if !task_file_path.fs_exists() {
        return false;
    }

    let Ok(task_file_content)
        = task_file_path.fs_read_text()
    else {
        return false;
    };

    let Ok(task_file)
        = parse(&task_file_content)
    else {
        return false;
    };

    task_file.tasks.contains_key(task_name.as_str())
}
