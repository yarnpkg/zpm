use std::{collections::HashSet, io::Write, os::unix::process::ExitStatusExt, process::ExitStatus};

use clipanion::cli;
use zpm_switch::{DaemonNotification, SubscriptionKind, TaskSubscription};
use zpm_tasks::{parse, TaskName};

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

        run_task_impl(&project, &self.name, &self.args, self.verbose_level).await
    }
}

pub async fn run_task(
    project: &Project,
    name: &str,
    _args: &[String],
    verbose_level: u8,
    _silent_dependencies: bool,
    _interlaced: bool,
    _enable_timers: bool,
) -> Result<ExitStatus, Error> {
    run_task_impl(project, name, _args, verbose_level).await
}

async fn run_task_impl(
    project: &Project,
    name: &str,
    _args: &[String],
    verbose_level: u8,
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
    let mut client = DaemonClient::connect().await?;

    // Push task with output and status subscriptions
    let task_subscriptions = vec![TaskSubscription {
        name: name.to_string(),
        subscriptions: vec![SubscriptionKind::Output, SubscriptionKind::Status],
    }];

    let task_ids = client.push_tasks(task_subscriptions, None).await?;

    if task_ids.is_empty() {
        return Err(Error::TaskPushFailed("No tasks enqueued".to_string()));
    }

    let target_task_ids: HashSet<String> = task_ids.into_iter().collect();

    // Listen for notifications until all target tasks complete
    let mut completed_tasks: HashSet<String> = HashSet::new();
    let mut exit_code = 0;

    loop {
        let notification = client.recv_notification().await?;

        match notification {
            DaemonNotification::TaskOutput { task_id, line, stream: _ } => {
                if verbose_level >= 1 {
                    let mut stdout = std::io::stdout().lock();
                    writeln!(stdout, "[{}] {}", task_id, line).ok();
                } else {
                    let mut stdout = std::io::stdout().lock();
                    writeln!(stdout, "{}", line).ok();
                }
            }
            DaemonNotification::TaskStarted { task_id } => {
                if verbose_level >= 2 {
                    let mut stdout = std::io::stdout().lock();
                    writeln!(stdout, "[{}] Process started", task_id).ok();
                }
            }
            DaemonNotification::TaskCompleted { task_id, exit_code: code } => {
                if target_task_ids.contains(&task_id) {
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
                if target_task_ids.contains(&task_id) {
                    return Err(Error::IpcError(format!("Task {} failed: {}", task_id, error)));
                }
            }
        }
    }

    Ok(ExitStatus::from_raw(exit_code))
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
