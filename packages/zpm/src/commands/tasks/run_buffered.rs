use std::io::Write;
use std::process::ExitStatus;

use async_trait::async_trait;
use clipanion::cli;

use super::helpers::format_task_id;
use super::runner::{run_task, TaskRunConfig, TaskRunContext, TaskRunHandler};
use crate::daemon::SubscriptionScope;
use crate::error::Error;

struct BufferedHandler;

#[async_trait]
impl TaskRunHandler for BufferedHandler {
    fn config(&self) -> TaskRunConfig {
        TaskRunConfig {
            output_subscription: SubscriptionScope::None,
            status_subscription: SubscriptionScope::FullTree,
        }
    }

    async fn on_output_line(&mut self, _ctx: &mut TaskRunContext, _task_id: &str, _line: &str) {}

    async fn on_task_started(&mut self, ctx: &mut TaskRunContext, task_id: &str, _is_target: bool) {
        if ctx.verbose_level >= 2 {
            let mut stdout
                = std::io::stdout().lock();

            writeln!(stdout, "[{}]: Process started", format_task_id(task_id)).ok();
        }
    }

    async fn on_task_completed(
        &mut self,
        ctx: &mut TaskRunContext,
        task_id: &str,
        exit_code: i32,
        _is_target: bool,
    ) {
        if let Ok(lines) = ctx.client.get_task_output(task_id).await {
            let mut stdout
                = std::io::stdout().lock();

            if !lines.is_empty() {
                if ctx.is_first_line {
                    if ctx.has_attached() {
                        writeln!(stdout, "").ok();
                    }

                    ctx.is_first_line = false;
                }

                for output_line in lines {
                    if ctx.verbose_level >= 1 {
                        writeln!(stdout, "[{}]: {}", format_task_id(task_id), output_line.line).ok();
                    } else {
                        writeln!(stdout, "{}", output_line.line).ok();
                    }
                }
            }
        }

        if ctx.verbose_level >= 2 {
            let mut stdout
                = std::io::stdout().lock();

            writeln!(stdout, "[{}]: Process exited (exit code {})", format_task_id(task_id), exit_code).ok();
        }
    }

    async fn on_task_failed(
        &mut self,
        _ctx: &mut TaskRunContext,
        task_id: &str,
        error: &str,
        is_target: bool,
    ) -> Option<Error> {
        if is_target {
            Some(Error::IpcError(format!("Task {} failed: {}", format_task_id(task_id), error)))
        } else {
            None
        }
    }

    fn on_ctrl_c(&mut self) {}
}

/// Run a task with buffered output
///
/// This command runs a task with buffered output mode. In this mode, the output
/// from each task (including dependencies) is collected and displayed only after
/// the task completes. This provides cleaner output when running multiple tasks
/// that might produce interleaved output.
///
/// The buffered mode is useful for CI environments or when you want to see the
/// complete output of each task as a unit rather than interleaved lines.
#[cli::command(proxy)]
#[cli::path("tasks", "run")]
#[cli::category("Task management commands")]
pub struct TaskRunBuffered {
    /// Enable buffered output mode
    #[cli::option("--buffered")]
    _buffered: bool,

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

impl TaskRunBuffered {
    pub async fn execute(&self) -> Result<ExitStatus, Error> {
        let mut handler
            = BufferedHandler;

        run_task(
            &mut handler,
            &self.name,
            &self.args,
            self.standalone,
            self.verbose_level,
        ).await
    }
}
