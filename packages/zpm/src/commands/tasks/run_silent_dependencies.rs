use std::{io::Write, sync::Arc};

use async_trait::async_trait;
use zpm_utils::{is_terminal, start_progress, ProgressHandle};

use super::helpers::format_task_id;
use super::runner::{TaskRunConfig, TaskRunContext, TaskRunHandler};
use crate::daemon::{ContextualTaskId, ProgressState, SubscriptionScope};

pub(super) struct SilentDependenciesHandler {
    pub progress_handle: Option<(ProgressHandle, Arc<ProgressState>)>,
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

    async fn on_output_line(&mut self, ctx: &mut TaskRunContext, _task_id: &ContextualTaskId, line: &str, _stream: &str) {
        let mut stdout
            = std::io::stdout().lock();

        ctx.emit_first_line_separator(&mut stdout);

        writeln!(stdout, "{}", line).ok();
    }

    async fn on_task_started(&mut self, _ctx: &mut TaskRunContext, task_id: &ContextualTaskId, is_target: bool) {
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
        task_id: &ContextualTaskId,
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
            // Target task failed.
            // Output was already printed live via on_output_line (TargetOnly subscription),
            // so do NOT replay it from the buffer — that would duplicate every line.
            self.stop_progress();
        }
    }

    async fn on_task_cancelled(
        &mut self,
        _ctx: &mut TaskRunContext,
        task_id: &ContextualTaskId,
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
