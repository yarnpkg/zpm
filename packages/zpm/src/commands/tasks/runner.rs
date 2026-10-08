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
use super::selection::TaskSelection;
use crate::daemon::{
    ContextualTaskId, DaemonClient, DaemonNotification, PushTasksOptions, PushTasksResult, StandaloneDaemonHandle,
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
            // Report the first failure (later cancellations would otherwise
            // overwrite its exit code)
            if code != 0 && self.exit_code == 0 {
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

    /// Called when the selection didn't match any task.
    fn on_nothing_to_run(&mut self) {
        eprintln!("No task matched the selection");
    }

    /// Called once all target tasks have completed.
    fn on_finished(&mut self, ctx: &TaskRunContext) {
        let _ = ctx;
    }
}

pub async fn run_task(
    handler: &mut impl TaskRunHandler,
    name: &str,
    args: &[String],
    standalone: bool,
    verbose_level: u8,
) -> Result<ExitStatus, Error> {
    run_tasks(handler, &[name.to_string()], args, &TaskRunOptions {
        standalone,
        verbose_level,
        ..TaskRunOptions::default()
    }).await
}

/// Options for `run_tasks`. The default selection runs the tasks in the
/// active workspace only.
#[derive(Default)]
pub struct TaskRunOptions<'a> {
    pub selection: TaskSelection<'a>,
    pub standalone: bool,
    pub verbose_level: u8,
    pub only: bool,
    pub concurrency: Option<usize>,
    pub continue_on_error: bool,
}

pub async fn run_tasks(
    handler: &mut impl TaskRunHandler,
    names: &[String],
    args: &[String],
    options: &TaskRunOptions<'_>,
) -> Result<ExitStatus, Error> {
    let mut project
        = Project::new(None).await?;

    project.lazy_install().await?;

    let workspace
        = project.active_workspace()?;

    let workspace_name
        = workspace.name.to_file_string();

    let task_subscriptions = match options.selection.is_active() {
        false => names.iter().map(|name| TaskSubscription {
            name: name.clone(),
            args: args.to_vec(),
            workspace: None,
        }).collect::<Vec<_>>(),

        true => {
            let targets
                = options.selection.select_targets(&project, names).await?;

            if targets.is_empty() {
                handler.on_nothing_to_run();
                return Ok(exit_status_from_code(0));
            }

            targets.into_iter().map(|(workspace, name)| TaskSubscription {
                name,
                args: args.to_vec(),
                workspace: Some(workspace.to_file_string()),
            }).collect::<Vec<_>>()
        },
    };

    let project_cwd
        = project.project_cwd.clone();

    let daemon_handle: Option<StandaloneDaemonHandle>;

    let mut client = if options.standalone {
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

    let config
        = handler.config();

    let name
        = names.join(" ");

    let push_options = PushTasksOptions {
        only: options.only,
        concurrency: options.concurrency,
    };

    let mut ctx = TaskRunContext {
        result: client
            .push_tasks_with_subscriptions(
                task_subscriptions,
                None,
                Some(workspace_name),
                config.output_subscription,
                config.status_subscription,
                Some(context_id),
                push_options,
            )
            .await?,
        client,
        target_task_ids: HashSet::new(),
        completed_tasks: HashSet::new(),
        exit_code: 0,
        is_first_line: true,
        verbose_level: options.verbose_level,
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

    // Single-task runs keep their historical behavior (only dependents of a
    // failed task are cancelled); multi-workspace runs default to fail-fast.
    let fail_fast
        = !options.continue_on_error && (options.selection.is_active() || names.len() > 1);

    let mut has_cancelled_context
        = false;

    #[cfg(unix)]
    let mut sigint
        = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;

    // CI runners (and `timeout`) stop jobs with SIGTERM; treat it like an
    // interrupt that always cancels the run, even for long-lived targets,
    // so no task process outlives the job.
    #[cfg(unix)]
    let mut sigterm
        = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;

    let is_standalone
        = options.standalone;

    loop {
        let notification
            = tokio::select! {
                biased;

                is_sigterm = async {
                    #[cfg(unix)]
                    {
                        tokio::select! {
                            _ = sigint.recv() => false,
                            _ = sigterm.recv() => true,
                        }
                    }

                    #[cfg(not(unix))]
                    {
                        tokio::signal::ctrl_c().await.ok();
                        false
                    }
                } => {
                    // Standalone daemons die with this process, so long-lived
                    // tasks can't be detached from; stop them instead.
                    if ctx.has_long_lived_target() && !is_standalone && !is_sigterm {
                        handler.on_ctrl_c();

                        println!();

                        if !ctx.is_first_line {
                            println!();
                        }

                        print_detach_footer(&name);

                        ctx.client.close();

                        if let Some(mut handle) = daemon_handle {
                            handle.shutdown().await;
                        }

                        return Ok(exit_status_from_code(0));
                    } else {
                        // Cancel all tasks in this context
                        handler.on_ctrl_c();

                        let _ = ctx.client.cancel_context(&context_id_for_cancel).await;

                        // Long-lived tasks live in their own context; stop
                        // the ones we started (the standalone daemon shutdown
                        // below kills them anyway).
                        let long_lived_targets: Vec<ContextualTaskId>
                            = ctx.target_task_ids.iter()
                                .filter(|id| is_long_lived_task(id))
                                .cloned()
                                .collect();

                        for target in long_lived_targets {
                            let _ = ctx.client.stop_task(target.task_id.task_name.as_str(), Some(target.task_id.workspace.to_file_string())).await;
                        }

                        ctx.client.close();

                        if let Some(mut handle) = daemon_handle {
                            handle.shutdown().await;
                        }

                        // Exit with the signal code (128 + SIGINT/SIGTERM)
                        return Ok(exit_status_from_code(if is_sigterm {143} else {130}));
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

                // Turbo semantics: by default the first failure stops the
                // run (running tasks are killed, pending ones cancelled);
                // `--continue` lets independent tasks finish.
                if exit_code != 0 && fail_fast && !has_cancelled_context {
                    has_cancelled_context = true;
                    let _ = ctx.client.cancel_context(&context_id_for_cancel).await;
                }

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
            DaemonNotification::FileChanged { .. } => {}
        }
    }

    handler.on_finished(&ctx);

    ctx.client.close();

    if let Some(mut handle) = daemon_handle {
        handle.shutdown().await;
    }

    Ok(exit_status_from_code(ctx.exit_code))
}
