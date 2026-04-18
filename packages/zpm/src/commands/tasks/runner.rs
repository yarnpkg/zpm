use std::{collections::HashSet, io::Write, os::unix::process::ExitStatusExt, process::ExitStatus, sync::Arc};

/// Create an ExitStatus from a logical exit code.
/// On Unix, `from_raw` expects a wait status where the exit code is in bits 8-15.
pub fn exit_status_from_code(code: i32) -> ExitStatus {
    ExitStatus::from_raw(code << 8)
}

use async_trait::async_trait;
use uuid::Uuid;
use zpm_utils::ToFileString;

use super::helpers::{is_long_lived_task, print_attach_header, print_detach_footer};
use crate::daemon::{
    ContextualTaskId, DaemonClient, DaemonNotification, PushTasksResult, StandaloneDaemonHandle,
    SubscriptionScope, TaskSubscription,
};
use crate::error::Error;
use crate::project::Project;

pub struct TaskRunConfig {
    pub output_subscription: SubscriptionScope,
    pub status_subscription: SubscriptionScope,
}

pub struct TaskRunContext {
    pub client: DaemonClient,
    pub result: PushTasksResult,
    pub target_task_ids: HashSet<ContextualTaskId>,
    pub completed_tasks: HashSet<ContextualTaskId>,
    pub exit_code: i32,
    pub is_first_line: bool,
    pub verbose_level: u8,
}

impl TaskRunContext {
    pub fn has_attached(&self) -> bool {
        !self.result.attached_long_lived.is_empty()
    }

    pub fn has_long_lived_target(&self) -> bool {
        self.target_task_ids.iter().any(|id| is_long_lived_task(id))
    }

    pub fn emit_first_line_separator(&mut self, stdout: &mut std::io::StdoutLock) {
        if self.is_first_line {
            if self.has_attached() {
                writeln!(stdout, "").ok();
            }
            self.is_first_line = false;
        }
    }

    pub fn is_target(&self, task_id: &ContextualTaskId) -> bool {
        self.target_task_ids.contains(task_id)
    }

    pub fn mark_completed(&mut self, task_id: ContextualTaskId, code: i32) {
        if self.target_task_ids.contains(&task_id) {
            self.completed_tasks.insert(task_id);
            if code != 0 {
                self.exit_code = code;
            }
        }
    }

    pub fn all_completed(&self) -> bool {
        self.completed_tasks.len() >= self.target_task_ids.len()
    }
}

#[async_trait]
pub trait TaskRunHandler: Send {
    fn config(&self) -> TaskRunConfig;

    fn on_tasks_pushed(&mut self, ctx: &TaskRunContext) {
        let _ = ctx;
    }

    async fn on_output_line(&mut self, ctx: &mut TaskRunContext, task_id: &ContextualTaskId, line: &str, stream: &str);

    async fn on_task_started(&mut self, ctx: &mut TaskRunContext, task_id: &ContextualTaskId, is_target: bool);

    async fn on_task_completed(
        &mut self,
        ctx: &mut TaskRunContext,
        task_id: &ContextualTaskId,
        exit_code: i32,
        is_target: bool,
    );

    async fn on_task_cancelled(
        &mut self,
        ctx: &mut TaskRunContext,
        task_id: &ContextualTaskId,
        is_target: bool,
    ) {
        let _ = (ctx, task_id, is_target);
    }

    fn on_ctrl_c(&mut self);
}

pub async fn run_task(
    handler: &mut impl TaskRunHandler,
    name: &str,
    args: &[String],
    standalone: bool,
    verbose_level: u8,
) -> Result<ExitStatus, Error> {
    let mut project
        = Project::new(None).await?;

    project.lazy_install().await?;

    let workspace
        = project.active_workspace()?;

    let workspace_name
        = workspace.name.to_file_string();

    let project_cwd
        = project.project_cwd.clone();

    let mut daemon_handle: Option<StandaloneDaemonHandle> = None;

    let mut client = if standalone {
        let project
            = Arc::new(project);

        let (c, handle)
            = DaemonClient::connect_standalone(project).await?;

        daemon_handle = Some(handle);
        c
    } else {
        daemon_handle = None;
        DaemonClient::connect(&project_cwd).await?
    };

    let context_id
        = Uuid::new_v4().to_string();

    let context_id_for_cancel
        = context_id.clone();

    let task_subscriptions = vec![TaskSubscription {
        name: name.to_string(),
        args: args.to_vec(),
    }];

    let config
        = handler.config();

    let mut ctx = TaskRunContext {
        result: client
            .push_tasks_with_subscriptions(
                task_subscriptions,
                None,
                Some(workspace_name),
                config.output_subscription,
                config.status_subscription,
                Some(context_id),
            )
            .await?,
        client,
        target_task_ids: HashSet::new(),
        completed_tasks: HashSet::new(),
        exit_code: 0,
        is_first_line: true,
        verbose_level,
    };

    if ctx.result.task_ids.is_empty() {
        return Err(Error::TaskPushFailed("No tasks enqueued".to_string()));
    }

    for attached in &ctx.result.attached_long_lived {
        print_attach_header(attached);
    }

    ctx.target_task_ids
        = ctx.result.task_ids.clone().into_iter().collect();

    handler.on_tasks_pushed(&ctx);

    loop {
        let notification
            = tokio::select! {
                biased;

                _ = tokio::signal::ctrl_c() => {
                    if ctx.has_long_lived_target() {
                        handler.on_ctrl_c();

                        println!();

                        if !ctx.is_first_line {
                            println!();
                        }

                        print_detach_footer(name);

                        ctx.client.close();

                        if let Some(mut handle) = daemon_handle {
                            handle.shutdown().await;
                        }

                        return Ok(exit_status_from_code(0));
                    } else {
                        // Cancel all tasks in this context
                        handler.on_ctrl_c();

                        let _ = ctx.client.cancel_context(&context_id_for_cancel).await;

                        ctx.client.close();

                        if let Some(mut handle) = daemon_handle {
                            handle.shutdown().await;
                        }

                        // Exit with SIGINT code (130 = 128 + 2)
                        return Ok(exit_status_from_code(130));
                    }
                }
                n = ctx.client.recv_notification() => n?,
            };

        match notification {
            DaemonNotification::TaskOutputLine { task_id, line, stream } => {
                handler.on_output_line(&mut ctx, &task_id, &line, &stream).await;
            }

            DaemonNotification::TaskStarted { task_id } => {
                let is_target
                    = ctx.is_target(&task_id);

                handler.on_task_started(&mut ctx, &task_id, is_target).await;
            }

            DaemonNotification::TaskCompleted { task_id, exit_code, .. } => {
                let is_target
                    = ctx.is_target(&task_id);

                handler
                    .on_task_completed(&mut ctx, &task_id, exit_code, is_target)
                    .await;

                ctx.mark_completed(task_id, exit_code);

                if ctx.all_completed() {
                    break;
                }
            }

            DaemonNotification::TaskCancelled { task_id } => {
                let is_target
                    = ctx.is_target(&task_id);

                handler
                    .on_task_cancelled(&mut ctx, &task_id, is_target)
                    .await;

                ctx.mark_completed(task_id, 1);

                if ctx.all_completed() {
                    break;
                }
            }

            DaemonNotification::TaskWarmUpComplete { .. } => {}
            DaemonNotification::DeclaredTasksChanged { .. } => {}
        }
    }

    ctx.client.close();

    if let Some(mut handle) = daemon_handle {
        handle.shutdown().await;
    }

    Ok(exit_status_from_code(ctx.exit_code))
}
