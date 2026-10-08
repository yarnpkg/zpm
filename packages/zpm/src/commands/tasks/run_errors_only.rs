use std::io::Write;

use async_trait::async_trait;

use super::helpers::format_task_id;
use super::runner::{TaskRunConfig, TaskRunContext, TaskRunHandler};
use crate::daemon::{ContextualTaskId, SubscriptionScope};

/// Only prints the (buffered) output of the tasks that failed, prefixed by
/// their task id, followed by a summary line. Equivalent to turbo's
/// `--output-logs=errors-only`.
pub(super) struct ErrorsOnlyHandler {
    pub succeeded: usize,
    pub failed: Vec<String>,
    pub cancelled: usize,
}

#[async_trait]
impl TaskRunHandler for ErrorsOnlyHandler {
    fn config(&self) -> TaskRunConfig {
        TaskRunConfig {
            output_subscription: SubscriptionScope::None,
            status_subscription: SubscriptionScope::FullTree,
        }
    }

    async fn on_output_line(&mut self, _ctx: &mut TaskRunContext, _task_id: &ContextualTaskId, _line: &str, _stream: &str) {}

    async fn on_task_started(&mut self, _ctx: &mut TaskRunContext, _task_id: &ContextualTaskId, _is_target: bool) {}

    async fn on_task_completed(
        &mut self,
        ctx: &mut TaskRunContext,
        task_id: &ContextualTaskId,
        exit_code: i32,
        _is_target: bool,
    ) {
        if exit_code == 0 {
            self.succeeded += 1;
            return;
        }

        self.failed.push(format_task_id(task_id));

        let lines
            = ctx.client.get_task_output(task_id).await.unwrap_or_default();

        let mut stdout
            = std::io::stdout().lock();

        for output_line in lines {
            writeln!(stdout, "[{}]: {}", format_task_id(task_id), output_line.line).ok();
        }

        writeln!(stdout, "[{}]: Process exited (exit code {})", format_task_id(task_id), exit_code).ok();
    }

    async fn on_task_cancelled(
        &mut self,
        _ctx: &mut TaskRunContext,
        _task_id: &ContextualTaskId,
        _is_target: bool,
    ) {
        self.cancelled += 1;
    }

    async fn on_task_cache_hit(&mut self, _ctx: &mut TaskRunContext, _task_id: &ContextualTaskId, _is_target: bool) {}

    fn on_ctrl_c(&mut self) {}

    fn on_nothing_to_run(&mut self) {
        println!("No task matched the selection");
    }

    fn on_finished(&mut self, _ctx: &TaskRunContext) {
        let mut stdout
            = std::io::stdout().lock();

        if self.failed.is_empty() {
            writeln!(stdout, "{} tasks succeeded", self.succeeded).ok();
            return;
        }

        writeln!(stdout, "{} tasks succeeded, {} failed, {} cancelled", self.succeeded, self.failed.len(), self.cancelled).ok();

        for task in &self.failed {
            writeln!(stdout, "  failed: {}", task).ok();
        }
    }
}
