use std::io::Write;
use std::process::ExitStatus;

use async_trait::async_trait;
use clipanion::cli;
use serde_json::json;

use super::helpers::{format_task_id, format_timestamp};
use super::runner::{run_task, TaskRunConfig, TaskRunContext, TaskRunHandler};
use crate::daemon::{ContextualTaskId, SubscriptionScope};
use crate::error::Error;

struct InterlacedHandler {
    timestamps: bool,
    json: bool,
}

#[async_trait]
impl TaskRunHandler for InterlacedHandler {
    fn config(&self) -> TaskRunConfig {
        TaskRunConfig {
            output_subscription: SubscriptionScope::FullTree,
            status_subscription: SubscriptionScope::FullTree,
        }
    }

    async fn on_output_line(&mut self, ctx: &mut TaskRunContext, task_id: &ContextualTaskId, line: &str, stream: &str) {
        let mut stdout
            = std::io::stdout().lock();

        if self.json {
            writeln!(stdout, "{}", json!({
                "type": "output",
                "taskId": format_task_id(task_id),
                "stream": stream,
                "line": line,
            })).ok();
            return;
        }

        ctx.emit_first_line_separator(&mut stdout);

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

    async fn on_task_started(&mut self, ctx: &mut TaskRunContext, task_id: &ContextualTaskId, _is_target: bool) {
        if self.json {
            let mut stdout
                = std::io::stdout().lock();

            writeln!(stdout, "{}", json!({
                "type": "task-started",
                "taskId": format_task_id(task_id),
            })).ok();
            return;
        }

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
        task_id: &ContextualTaskId,
        exit_code: i32,
        _is_target: bool,
    ) {
        if self.json {
            let mut stdout
                = std::io::stdout().lock();

            writeln!(stdout, "{}", json!({
                "type": "task-completed",
                "taskId": format_task_id(task_id),
                "exitCode": exit_code,
            })).ok();
            return;
        }

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

    async fn on_task_cancelled(
        &mut self,
        _ctx: &mut TaskRunContext,
        task_id: &ContextualTaskId,
        _is_target: bool,
    ) {
        if self.json {
            let mut stdout
                = std::io::stdout().lock();

            writeln!(stdout, "{}", json!({
                "type": "task-cancelled",
                "taskId": format_task_id(task_id),
            })).ok();
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

    /// Output JSON objects (one per line) for each task event
    #[cli::option("--json", default = false)]
    json: bool,

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
                json: self.json,
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
