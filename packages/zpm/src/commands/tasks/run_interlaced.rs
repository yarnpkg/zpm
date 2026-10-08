use std::io::Write;

use async_trait::async_trait;
use serde_json::json;

use super::helpers::{format_task_id, format_timestamp};
use super::runner::{TaskRunConfig, TaskRunContext, TaskRunHandler};
use crate::daemon::{ContextualTaskId, SubscriptionScope};

pub(super) struct InterlacedHandler {
    pub timestamps: bool,
    pub json: bool,
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

    async fn on_task_cache_hit(&mut self, ctx: &mut TaskRunContext, task_id: &ContextualTaskId, _is_target: bool) {
        if self.json {
            let mut stdout
                = std::io::stdout().lock();

            writeln!(stdout, "{}", json!({
                "type": "task-cache-hit",
                "taskId": format_task_id(task_id),
            })).ok();
            return;
        }

        super::runner::print_cache_hit(ctx, task_id);
    }

    fn on_ctrl_c(&mut self) {}
}
