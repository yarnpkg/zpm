// ============================================================================
// Coordinator State (Composed)
//
// This module consolidates all mutable daemon state into sub-structs
// that are owned exclusively by the coordinator loop. No Arc<RwLock> wrappers —
// the coordinator is the single owner, making race conditions structurally
// impossible.
//
// Every terminal transition goes through `close_task`, which updates all
// relevant registries atomically. The coordinator loop calls transition
// methods that return `TransitionEffects` describing what I/O to perform,
// keeping this module free of channels, Tokio types, and process management.
// ============================================================================

mod long_lived_registry;
mod output_buffer;
mod process_registry;
mod subscription_manager;
mod task_graph;

pub use long_lived_registry::LongLivedRegistry;
pub use output_buffer::OutputBuffer;
pub use process_registry::ProcessRegistry;
pub use subscription_manager::{SubscriptionId, SubscriptionManager};
pub use task_graph::{format_contextual_task_id, TaskGraph};

// Re-export scheduler types that are used across the coordinator
pub use super::scheduler::{ContextualTaskId, PreparedTask};

use super::ipc::DaemonNotification;

// ============================================================================
// Transition Effects
// ============================================================================

/// Describes the I/O the coordinator loop should perform after a transition.
/// Keeps CoordinatorState free of channels, Tokio types, and process management.
#[derive(Default)]
pub struct TransitionEffects {
    pub notifications: Vec<DaemonNotification>,
    pub pids_to_kill: Vec<u32>,
}

// ============================================================================
// Close Task Effect (internal)
// ============================================================================

struct CloseTaskEffect {
    task_id_str: String,
    pid: Option<u32>,
}

// ============================================================================
// Coordinator State
// ============================================================================

/// All mutable daemon state, composed from focused sub-structs.
/// Only modified by the coordinator event loop — no locks needed.
pub struct CoordinatorState {
    pub graph: TaskGraph,
    pub processes: ProcessRegistry,
    pub long_lived: LongLivedRegistry,
    pub subscriptions: SubscriptionManager,
    pub output: OutputBuffer,
}

impl CoordinatorState {
    pub fn new(output_buffer_max_lines: usize, max_closed_tasks: usize) -> Self {
        Self {
            graph: TaskGraph::new(),
            processes: ProcessRegistry::new(),
            long_lived: LongLivedRegistry::new(),
            subscriptions: SubscriptionManager::new(),
            output: OutputBuffer::new(output_buffer_max_lines, max_closed_tasks),
        }
    }

    // ========================================================================
    // Core: close_task (single cleanup codepath for all terminal transitions)
    // ========================================================================

    /// Clean up all registries for a task that has reached a terminal state.
    /// Called by every transition that ends a task (complete, fail, cancel).
    fn close_task(&mut self, task_id: &ContextualTaskId) -> CloseTaskEffect {
        let task_id_str = format_contextual_task_id(task_id);

        // 1. Output buffer: mark closed, may trigger eviction of old closed tasks
        let evicted = self.output.mark_closed(task_id_str.clone());
        for id in &evicted {
            self.graph.evict_closed_task(id);
        }

        // 2. Long-lived registry (no-op if not long-lived)
        self.long_lived.remove(&task_id.task_id);

        // 3. Process registry: take PID if still registered
        let pid = self.processes.take_pid_for_task(task_id);

        CloseTaskEffect { task_id_str, pid }
    }

    // ========================================================================
    // Transition: task_script_finished
    // ========================================================================

    /// Called when a task's script exits. Routes to complete_task or fail_task
    /// based on exit code and subtask state.
    pub fn task_script_finished(
        &mut self,
        task_id: &ContextualTaskId,
        exit_code: i32,
    ) -> TransitionEffects {
        self.graph.mark_script_finished_with_code(task_id, exit_code);

        if exit_code != 0 {
            self.fail_task(task_id, exit_code)
        } else {
            self.try_complete_or_wait(task_id, exit_code)
        }
    }

    /// Called when a task's script exits with success. Checks subtask state
    /// to determine whether to complete or wait.
    fn try_complete_or_wait(
        &mut self,
        task_id: &ContextualTaskId,
        exit_code: i32,
    ) -> TransitionEffects {
        if self.graph.try_complete_task(task_id) {
            self.on_task_completed(task_id, exit_code)
        } else if self.graph.has_failed_subtask(task_id) {
            // Subtask already failed - fail the parent
            self.fail_task(task_id, 1)
        } else {
            // Task stays in WaitingForSubtasks until all subtasks complete
            TransitionEffects::default()
        }
    }

    /// Common completion path: broadcast + close + try to complete parents.
    fn on_task_completed(
        &mut self,
        task_id: &ContextualTaskId,
        exit_code: i32,
    ) -> TransitionEffects {
        let mut effects = TransitionEffects::default();

        let close = self.close_task(task_id);
        effects.notifications.push(DaemonNotification::TaskCompleted {
            task_id: close.task_id_str,
            exit_code,
        });
        if let Some(pid) = close.pid {
            effects.pids_to_kill.push(pid);
        }

        // Try to complete parents that are waiting for subtasks
        let parents = self.graph.find_parents(task_id);
        for parent in parents {
            if let Some(parent_exit_code) = self.graph.get_waiting_exit_code(&parent) {
                if self.graph.try_complete_task(&parent) {
                    let parent_effects = self.on_task_completed(&parent, parent_exit_code);
                    effects.notifications.extend(parent_effects.notifications);
                    effects.pids_to_kill.extend(parent_effects.pids_to_kill);
                }
            }
        }

        effects
    }

    // ========================================================================
    // Transition: fail_task
    // ========================================================================

    /// Mark a task as failed, close it, and propagate failure to waiting parents.
    pub fn fail_task(
        &mut self,
        task_id: &ContextualTaskId,
        exit_code: i32,
    ) -> TransitionEffects {
        let mut effects = TransitionEffects::default();

        self.graph.mark_failed(task_id);

        let close = self.close_task(task_id);
        effects.notifications.push(DaemonNotification::TaskCompleted {
            task_id: close.task_id_str,
            exit_code,
        });
        if let Some(pid) = close.pid {
            effects.pids_to_kill.push(pid);
        }

        // Propagate failure to parents that are waiting for subtasks
        let parents = self.graph.find_parents(task_id);
        for parent in parents {
            if self.graph.get_waiting_exit_code(&parent).is_some() {
                let parent_effects = self.fail_task(&parent, exit_code);
                effects.notifications.extend(parent_effects.notifications);
                effects.pids_to_kill.extend(parent_effects.pids_to_kill);
            }
        }

        effects
    }

    // ========================================================================
    // Transition: cancel_task
    // ========================================================================

    /// Mark a single task as cancelled, close it, and return effects.
    pub fn cancel_task(
        &mut self,
        task_id: &ContextualTaskId,
    ) -> TransitionEffects {
        let mut effects = TransitionEffects::default();

        self.graph.mark_cancelled(task_id);

        let close = self.close_task(task_id);
        effects.notifications.push(DaemonNotification::TaskCancelled {
            task_id: close.task_id_str,
        });
        if let Some(pid) = close.pid {
            effects.pids_to_kill.push(pid);
        }

        effects
    }

    // ========================================================================
    // Transition: cancel_context
    // ========================================================================

    /// Cancel all non-terminal tasks in a context. Collects PIDs to kill
    /// and marks spawning tasks for deferred kill.
    pub fn cancel_context(&mut self, context_id: &str) -> TransitionEffects {
        let mut effects = TransitionEffects::default();

        // 1. Cancel all non-terminal tasks in graph
        let tasks_to_cancel: Vec<ContextualTaskId> = self.graph
            .prepared
            .keys()
            .filter(|ctx_task_id| {
                ctx_task_id.context_id == context_id && !self.graph.is_terminal(ctx_task_id)
            })
            .cloned()
            .collect();

        for task_id in &tasks_to_cancel {
            let task_effects = self.cancel_task(task_id);
            effects.notifications.extend(task_effects.notifications);
            effects.pids_to_kill.extend(task_effects.pids_to_kill);
        }

        // 2. Get and collect registered PIDs for running tasks in this context
        let pids = self.processes.take_pids_for_context(context_id);
        effects.pids_to_kill.extend(pids);

        // 3. Mark spawning tasks for deferred kill
        let spawning_ids = self.processes.get_spawning_for_context(context_id);
        for task_id in &spawning_ids {
            self.processes.mark_spawning_pending_cancel(task_id);
        }

        effects
    }

    // ========================================================================
    // Transition: warm_up_complete
    // ========================================================================

    /// Handle warm-up completion for a long-lived task.
    /// Returns empty effects if the task is already terminal (guards against
    /// the timer firing after task failure).
    pub fn warm_up_complete(
        &mut self,
        task_id: &ContextualTaskId,
        base_task_id: &zpm_tasks::TaskId,
    ) -> TransitionEffects {
        let mut effects = TransitionEffects::default();

        // Guard: if task already died, ignore the warm-up timer
        if self.graph.is_terminal(task_id) {
            return effects;
        }

        self.graph.mark_warm_up_complete(task_id);
        self.long_lived.mark_warm_up_complete(base_task_id);

        let task_id_str = format_contextual_task_id(task_id);
        effects.notifications.push(DaemonNotification::TaskWarmUpComplete {
            task_id: task_id_str,
        });

        effects
    }

    // ========================================================================
    // Transition: stop_long_lived
    // ========================================================================

    /// Stop a long-lived task by task ID. Cleans up registries and returns
    /// the PID to kill (if any).
    pub fn stop_long_lived(
        &mut self,
        task_id: &zpm_tasks::TaskId,
        contextual_task_id: &ContextualTaskId,
    ) -> TransitionEffects {
        let mut effects = TransitionEffects::default();

        // Check if spawning
        if self.processes.mark_spawning_pending_cancel(contextual_task_id) {
            self.long_lived.remove(task_id);
            return effects;
        }

        // close_task handles output, long_lived, and process cleanup
        let close = self.close_task(contextual_task_id);
        if let Some(pid) = close.pid {
            effects.pids_to_kill.push(pid);
        }

        effects
    }

    // ========================================================================
    // Transition: complete_no_script (task with no script completes immediately)
    // ========================================================================

    /// Complete a task that has no script (dependency aggregator).
    pub fn complete_no_script(
        &mut self,
        task_id: &ContextualTaskId,
    ) -> TransitionEffects {
        self.graph.mark_completed(task_id);
        let mut effects = TransitionEffects::default();

        let close = self.close_task(task_id);
        effects.notifications.push(DaemonNotification::TaskCompleted {
            task_id: close.task_id_str,
            exit_code: 0,
        });

        effects
    }
}
