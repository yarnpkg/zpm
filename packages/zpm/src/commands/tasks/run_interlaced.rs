use std::io::Write;
use std::process::ExitStatus;

use async_trait::async_trait;
use clipanion::cli;

use super::helpers::{format_task_id, format_timestamp};
use super::runner::{run_task, TaskRunConfig, TaskRunContext, TaskRunHandler};
use crate::daemon::SubscriptionScope;
use crate::error::Error;

struct InterlacedHandler {
    timestamps: bool,
}

#[async_trait]
impl TaskRunHandler for InterlacedHandler {
    fn config(&self) -> TaskRunConfig {
        TaskRunConfig {
            output_subscription: SubscriptionScope::FullTree,
            status_subscription: SubscriptionScope::FullTree,
        }
    }

    async fn on_output_line(&mut self, ctx: &mut TaskRunContext, task_id: &str, line: &str) {
        let mut stdout
            = std::io::stdout().lock();

        if ctx.is_first_line {
            if ctx.has_attached() {
                writeln!(stdout, "").ok();
            }

            ctx.is_first_line = false;
        }

        if self.timestamps {
            if ctx.verbose_level >= 1 {
                writeln!(stdout, "[{}] [{}]: {}", format_timestamp(), format_task_id(task_id), line).ok();
            } else {
                writeln!(stdout, "[{}] {}", format_timestamp(), line).ok();
            }
        } else if ctx.verbose_level >= 1 {
            writeln!(stdout, "[{}]: {}", format_task_id(task_id), line).ok();
        } else {
            writeln!(stdout, "{}", line).ok();
        }
    }

    async fn on_task_started(&mut self, ctx: &mut TaskRunContext, task_id: &str, _is_target: bool) {
        if ctx.verbose_level >= 2 {
            let mut stdout
                = std::io::stdout().lock();

            if self.timestamps {
                writeln!(stdout, "[{}] [{}]: Process started", format_timestamp(), format_task_id(task_id)).ok();
            } else {
                writeln!(stdout, "[{}]: Process started", format_task_id(task_id)).ok();
            }
        }
    }

    async fn on_task_completed(
        &mut self,
        ctx: &mut TaskRunContext,
        task_id: &str,
        exit_code: i32,
        _is_target: bool,
    ) {
        if ctx.verbose_level >= 2 {
            let mut stdout
                = std::io::stdout().lock();

            if self.timestamps {
                writeln!(stdout, "[{}] [{}]: Process exited (exit code {})", format_timestamp(), format_task_id(task_id), exit_code).ok();
            } else {
                writeln!(stdout, "[{}]: Process exited (exit code {})", format_task_id(task_id), exit_code).ok();
            }
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

/// Run a task with interlaced output (default)
///
/// This command runs a task with interlaced output mode. In this mode, output
/// from the task and its dependencies is displayed in real-time as it is
/// produced. Lines from different tasks may be interleaved.
///
/// This is the default mode for running tasks and provides the most responsive
/// feedback during execution.
#[cli::command(proxy)]
#[cli::path("tasks", "run")]
#[cli::category("Task management commands")]
pub struct TaskRunInterlaced {
    /// Increase the verbosity level (can be repeated)
    #[cli::option("-v,--verbose", default = if zpm_utils::is_terminal() {2} else {0}, counter)]
    verbose_level: u8,

    /// Prefix each output line with a timestamp
    #[cli::option("--timestamps", default = false)]
    timestamps: bool,

    /// Run the task without connecting to the daemon
    #[cli::option("--standalone", default = false)]
    standalone: bool,

    /// Name of the task to run
    name: String,

    /// Arguments to pass to the task
    args: Vec<String>,
}

impl TaskRunInterlaced {
    pub async fn execute(&self) -> Result<ExitStatus, Error> {
        let mut handler
            = InterlacedHandler {
                timestamps: self.timestamps,
            };

        let x = run_task(
            &mut handler,
            &self.name,
            &self.args,
            self.standalone,
            self.verbose_level,
        ).await;

        x
    }
}
