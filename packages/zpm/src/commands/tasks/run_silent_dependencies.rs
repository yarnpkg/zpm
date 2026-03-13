use std::io::Write;
use std::process::ExitStatus;
use std::sync::Arc;

use async_trait::async_trait;
use clipanion::{Environment, cli};
use zpm_utils::{is_terminal, start_progress, ProgressHandle};

use super::helpers::format_task_id;
use super::runner::{run_task, TaskRunConfig, TaskRunContext, TaskRunHandler};
use crate::daemon::{ProgressState, SubscriptionScope};
use crate::error::Error;

struct SilentDependenciesHandler {
    progress_handle: Option<(ProgressHandle, Arc<ProgressState>)>,
}

impl SilentDependenciesHandler {
    fn stop_progress(&mut self) {
        if let Some((ref mut handle, _)) = self.progress_handle {
            handle.stop();
        }
    }
}

#[async_trait]
impl TaskRunHandler for SilentDependenciesHandler {
    fn config(&self) -> TaskRunConfig {
        TaskRunConfig {
            output_subscription: SubscriptionScope::TargetOnly,
            status_subscription: SubscriptionScope::FullTree,
        }
    }

    fn on_tasks_pushed(&mut self, ctx: &TaskRunContext) {
        let show_progress
            = is_terminal() && ctx.result.dependency_count > 0;

        if show_progress {
            let progress_state
                = Arc::new(ProgressState::new(ctx.result.dependency_count));

            let progress_state_clone
                = progress_state.clone();

            self.progress_handle = Some((
                start_progress(move |frame_idx| progress_state_clone.format_progress(frame_idx)),
                progress_state,
            ));
        }
    }

    async fn on_output_line(&mut self, ctx: &mut TaskRunContext, _task_id: &str, line: &str, _stream: &str) {
        let mut stdout
            = std::io::stdout().lock();

        if ctx.is_first_line {
            if ctx.has_attached() {
                writeln!(stdout, "").ok();
            }

            ctx.is_first_line = false;
        }

        writeln!(stdout, "{}", line).ok();
    }

    async fn on_task_started(&mut self, _ctx: &mut TaskRunContext, task_id: &str, is_target: bool) {
        if is_target {
            self.stop_progress();
        } else {
            if let Some((_, ref progress_state)) = self.progress_handle {
                progress_state.add_task(&format_task_id(task_id));
            }
        }
    }

    async fn on_task_completed(
        &mut self,
        ctx: &mut TaskRunContext,
        task_id: &str,
        exit_code: i32,
        is_target: bool,
    ) {
        if !is_target {
            if let Some((_, ref progress_state)) = self.progress_handle {
                progress_state.remove_task(&format_task_id(task_id));
            }

            if exit_code != 0 {
                self.stop_progress();

                let lines = ctx.client.get_task_output(task_id).await.ok();

                if lines.as_ref().map_or(false, |l| !l.is_empty()) {
                    let mut stdout = std::io::stdout().lock();

                    writeln!(stdout, "[{}]: Process started", format_task_id(task_id)).ok();

                    for output_line in lines.unwrap() {
                        writeln!(stdout, "[{}]: {}", format_task_id(task_id), output_line.line).ok();
                    }

                    writeln!(stdout, "[{}]: Process exited (exit code {})", format_task_id(task_id), exit_code).ok();
                }
            }
        } else if exit_code != 0 {
            // Target task failed - print its output
            self.stop_progress();

            let lines = ctx.client.get_task_output(task_id).await.ok();

            if lines.as_ref().map_or(false, |l| !l.is_empty()) {
                let mut stdout = std::io::stdout().lock();

                writeln!(stdout, "[{}]: Process started", format_task_id(task_id)).ok();

                for output_line in lines.unwrap() {
                    writeln!(stdout, "[{}]: {}", format_task_id(task_id), output_line.line).ok();
                }

                writeln!(stdout, "[{}]: Process exited (exit code {})", format_task_id(task_id), exit_code).ok();
            }
        }
    }

    async fn on_task_cancelled(
        &mut self,
        _ctx: &mut TaskRunContext,
        task_id: &str,
        is_target: bool,
    ) {
        if let Some((_, ref progress_state)) = self.progress_handle {
            progress_state.remove_task(&format_task_id(task_id));
        }

        if is_target {
            self.stop_progress();
        }
    }

    fn on_ctrl_c(&mut self) {
        self.stop_progress();
    }
}

/// Run a task with silent dependency output
///
/// This command runs a task while suppressing output from dependency tasks.
/// Only the output from the target task itself is shown, with a progress
/// indicator displayed while dependencies are running.
///
/// If a dependency task fails, its output will be displayed to help diagnose
/// the failure. This mode is useful when you're primarily interested in the
/// output of the main task and dependencies are expected to succeed silently.
#[cli::command(proxy)]
#[cli::path("tasks", "run")]
#[cli::category("Task management commands")]
pub struct TaskRunSilentDependencies {
    /// Enable silent dependencies mode
    #[cli::option("--silent-dependencies")]
    _silent_dependencies: bool,

    /// Increase the verbosity level (can be repeated)
    #[cli::option("-v,--verbose", default = if zpm_utils::is_terminal() {2} else {0}, counter)]
    verbose_level: u8,

    /// Run the task without connecting to the daemon
    #[cli::option("--standalone", default = false)]
    standalone: bool,

    /// Name of the task to run
    name: String,

    /// Arguments to pass to the task
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
        let mut handler
            = SilentDependenciesHandler {
                progress_handle: None,
            };

        run_task(
            &mut handler,
            &self.name,
            &self.args,
            self.standalone,
            self.verbose_level,
        ).await
    }
}
