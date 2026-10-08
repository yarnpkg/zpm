use std::io::Write;

use async_trait::async_trait;

use super::helpers::format_task_id;
use super::runner::{TaskRunConfig, TaskRunContext, TaskRunHandler};
use crate::daemon::{ContextualTaskId, SubscriptionScope};

pub(super) struct BufferedHandler;

#[async_trait]
impl TaskRunHandler for BufferedHandler {
    fn config(&self) -> TaskRunConfig {
        TaskRunConfig {
            output_subscription: SubscriptionScope::None,
            status_subscription: SubscriptionScope::FullTree,
        }
    }

    async fn on_output_line(&mut self, _ctx: &mut TaskRunContext, _task_id: &ContextualTaskId, _line: &str, _stream: &str) {}

    async fn on_task_started(&mut self, ctx: &mut TaskRunContext, task_id: &ContextualTaskId, _is_target: bool) {
        if ctx.verbose_level >= 2 {
            let mut stdout
                = std::io::stdout().lock();

            writeln!(stdout, "[{}]: Process started", format_task_id(task_id)).ok();
        }
    }

    async fn on_task_completed(
        &mut self,
        ctx: &mut TaskRunContext,
        task_id: &ContextualTaskId,
        exit_code: i32,
        _is_target: bool,
    ) {
        if let Ok(lines) = ctx.client.get_task_output(task_id).await {
            let mut stdout
                = std::io::stdout().lock();

            if !lines.is_empty() {
                ctx.emit_first_line_separator(&mut stdout);

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

    fn on_ctrl_c(&mut self) {}
}
