mod context_registry;
mod event_history;
mod file_watcher;
mod long_lived_registry;
mod output_buffer;
mod process_registry;
mod subscription_manager;
mod task_graph;
mod taskfile_watcher;

pub use context_registry::ContextRegistry;
pub use event_history::{EventHistory, now_ms};
pub use file_watcher::FileWatcher;
pub use long_lived_registry::LongLivedRegistry;
pub use output_buffer::OutputBuffer;
pub use process_registry::ProcessRegistry;
pub use subscription_manager::{SubscriptionId, SubscriptionManager};
pub use task_graph::{TaskGraph, LONG_LIVED_ATTRIBUTE};
pub use taskfile_watcher::TaskfileWatcher;

// Re-export scheduler types that are used across the coordinator
pub use super::scheduler::{ContextualTaskId, PreparedTask};

use std::collections::BTreeSet;

use zpm_tasks::TaskId;

use std::path::PathBuf;

use tokio::sync::mpsc;

use super::ipc::DaemonNotification;

/// Describes the I/O the coordinator loop should perform after a transition.
/// Keeps CoordinatorState free of channels, Tokio types, and process management.
#[derive(Default)]
pub struct TransitionEffects {
    pub notifications: Vec<DaemonNotification>,
    pub pids_to_kill: Vec<u32>,
}

struct CloseTaskEffect {
    task_id: ContextualTaskId,
    pid: Option<u32>,
}

/// All mutable daemon state, composed from focused sub-structs.
/// Only modified by the coordinator event loop — no locks needed.
pub struct CoordinatorState {
    pub graph: TaskGraph,
    pub contexts: ContextRegistry,
    pub processes: ProcessRegistry,
    pub long_lived: LongLivedRegistry,
    pub subscriptions: SubscriptionManager,
    pub output: OutputBuffer,
    pub event_history: EventHistory,
    pub taskfile_watcher: TaskfileWatcher,
    pub file_watcher: FileWatcher,
}

impl CoordinatorState {
    pub fn new(
        output_buffer_max_lines: usize,
        max_closed_tasks: usize,
        file_notify_tx: mpsc::UnboundedSender<notify::Event>,
        taskfile_notify_tx: mpsc::UnboundedSender<notify::Event>,
        project_cwd: PathBuf,
    ) -> Self {
        Self {
            graph: TaskGraph::new(),
            contexts: ContextRegistry::new(),
            processes: ProcessRegistry::new(),
            long_lived: LongLivedRegistry::new(),
            subscriptions: SubscriptionManager::new(),
            output: OutputBuffer::new(output_buffer_max_lines, max_closed_tasks),
            event_history: EventHistory::new(),
            taskfile_watcher: TaskfileWatcher::new(taskfile_notify_tx),
            file_watcher: FileWatcher::new(file_notify_tx, project_cwd),
        }
    }

    /// Clean up all registries for a task that has reached a terminal state.
    /// Called by every transition that ends a task (complete, fail, cancel).
    fn close_task(&mut self, task_id: &ContextualTaskId) -> CloseTaskEffect {
        // 1. Decrement the context's active task counter
        self.contexts.decrement(&task_id.context_id);

        // 2. Output buffer: mark closed, may trigger eviction of old closed tasks
        let evicted = self.output.mark_closed(task_id.clone());
        for id in &evicted {
            // Only evict from the graph if the task's context is no longer active.
            // While a context is active we must keep task entries so that
            // duplicate pushes of already-completed tasks can be detected.
            if !self.contexts.is_active(&id.context_id) {
                self.graph.evict_closed_task(id);
            }
        }

        // 3. Long-lived registry (no-op if not long-lived)
        self.long_lived.remove(&task_id.task_id);

        // 4. Process registry: take PID if still registered
        let pid = self.processes.take_pid_for_task(task_id);

        CloseTaskEffect { task_id: task_id.clone(), pid }
    }

    /// Called when a task's script exits. Routes to complete_task or fail_task
    /// based on exit code and subtask state.
    ///
    /// Guards against already-terminal tasks: if the task was stopped and
    /// then re-added before the old process's TaskCompleted arrived, the
    /// task_id now refers to the new run. Ignore the stale completion.
    pub fn task_script_finished(
        &mut self,
        task_id: &ContextualTaskId,
        exit_code: i32,
        signal: Option<i32>,
    ) -> TransitionEffects {
        // Guard: ignore completions for tasks that are already terminal
        // (e.g. a stopped task whose process finally exited after re-add)
        if self.graph.is_terminal(task_id) {
            return TransitionEffects::default();
        }

        self.graph.mark_script_finished_with_code(task_id, exit_code);

        if exit_code != 0 {
            self.fail_task(task_id, exit_code, signal)
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
        } else if let Some(subtask_exit_code) = self.graph.failed_subtask_exit_code(task_id) {
            // Subtask already failed - fail the parent (no signal for propagated failure)
            self.fail_task(task_id, subtask_exit_code, None)
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
            task_id: close.task_id,
            exit_code,
            signal: None,
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

    /// Mark a task as failed, close it, and propagate failure to waiting parents.
    pub fn fail_task(
        &mut self,
        task_id: &ContextualTaskId,
        exit_code: i32,
        signal: Option<i32>,
    ) -> TransitionEffects {
        let mut effects = TransitionEffects::default();

        self.graph.mark_failed(task_id, exit_code);

        let close = self.close_task(task_id);
        effects.notifications.push(DaemonNotification::TaskCompleted {
            task_id: close.task_id,
            exit_code,
            signal,
        });
        if let Some(pid) = close.pid {
            effects.pids_to_kill.push(pid);
        }

        // Propagate failure to parents that are waiting for subtasks
        // (propagated failures have no signal — only the original task does)
        let parents = self.graph.find_parents(task_id);
        for parent in parents {
            if self.graph.get_waiting_exit_code(&parent).is_some() {
                let parent_effects = self.fail_task(&parent, exit_code, None);
                effects.notifications.extend(parent_effects.notifications);
                effects.pids_to_kill.extend(parent_effects.pids_to_kill);
            }
        }

        effects
    }

    /// Mark a single task as cancelled, close it, and return effects.
    pub fn cancel_task(
        &mut self,
        task_id: &ContextualTaskId,
    ) -> TransitionEffects {
        let mut effects = TransitionEffects::default();

        self.graph.mark_cancelled(task_id);

        let close = self.close_task(task_id);
        effects.notifications.push(DaemonNotification::TaskCancelled {
            task_id: close.task_id,
        });
        if let Some(pid) = close.pid {
            effects.pids_to_kill.push(pid);
        }

        effects
    }

    /// Cancel all non-terminal tasks in a context. Collects PIDs to kill
    /// and marks spawning tasks for deferred kill.
    ///
    /// Tasks are cancelled in topological order (dependencies before dependents),
    /// with alphabetical ordering as tiebreaker within the same topological level.
    pub fn cancel_context(&mut self, context_id: &str) -> TransitionEffects {
        let mut effects = TransitionEffects::default();

        // 1. Collect all non-terminal tasks in this context
        let tasks_to_cancel: Vec<ContextualTaskId> = self.graph
            .prepared
            .keys()
            .filter(|ctx_task_id| {
                ctx_task_id.context_id == context_id && !self.graph.is_terminal(ctx_task_id)
            })
            .cloned()
            .collect();

        // 2. Sort in topological order (dependencies first, alphabetical tiebreak)
        let sorted = topological_sort(&tasks_to_cancel, &self.graph.resolved.tasks);

        for task_id in &sorted {
            let task_effects = self.cancel_task(task_id);
            effects.notifications.extend(task_effects.notifications);
            effects.pids_to_kill.extend(task_effects.pids_to_kill);
        }

        // 3. Get and collect registered PIDs for running tasks in this context
        let pids = self.processes.take_pids_for_context(context_id);
        effects.pids_to_kill.extend(pids);

        // 4. Mark spawning tasks for deferred kill
        let spawning_ids = self.processes.get_spawning_for_context(context_id);
        for task_id in &spawning_ids {
            self.processes.mark_spawning_pending_cancel(task_id);
        }

        effects
    }

    /// Handle warm-up completion for a long-lived task.
    /// Returns empty effects if the task is already terminal (guards against
    /// the timer firing after task failure).
    ///
    /// Stale timers from a previous run are prevented by aborting the timer's
    /// tokio task when the long-lived task is stopped (see `stop_long_lived`
    /// and `LongLivedRegistry::remove`).
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

        effects.notifications.push(DaemonNotification::TaskWarmUpComplete {
            task_id: task_id.clone(),
        });

        effects
    }

    /// Stop a long-lived task by task ID. Removes the long-lived registry
    /// entry and returns the PID to kill.
    ///
    /// Does NOT call close_task — the process is still alive after we send
    /// SIGTERM. When it actually exits, TaskCompleted will fire and the
    /// normal task_script_finished → fail_task → close_task path handles
    /// the full cleanup. This avoids double-eviction of output buffers
    /// and lets add_task re-register the task cleanly on restart.
    pub fn stop_long_lived(
        &mut self,
        task_id: &zpm_tasks::TaskId,
        contextual_task_id: &ContextualTaskId,
    ) -> TransitionEffects {
        let mut effects = TransitionEffects::default();

        // Remove from long-lived registry first so that a subsequent
        // PushTasks for the same task doesn't attach to the dying instance.
        self.long_lived.remove(task_id);

        // Check if spawning (between spawn and PID registration)
        if self.processes.mark_spawning_pending_cancel(contextual_task_id) {
            return effects;
        }

        // Take the PID so we can kill it, but leave graph/output state intact
        // for the TaskCompleted path to clean up properly.
        if let Some(pid) = self.processes.take_pid_for_task(contextual_task_id) {
            effects.pids_to_kill.push(pid);
        }

        effects
    }

    /// Complete a task that has no script (dependency aggregator).
    pub fn complete_no_script(
        &mut self,
        task_id: &ContextualTaskId,
    ) -> TransitionEffects {
        self.graph.mark_completed(task_id);
        let mut effects = TransitionEffects::default();

        let close = self.close_task(task_id);
        effects.notifications.push(DaemonNotification::TaskCompleted {
            task_id: close.task_id,
            exit_code: 0,
            signal: None,
        });

        effects
    }
}

/// Sort contextual task IDs in topological order (dependencies before dependents),
/// using alphabetical ordering as tiebreaker within the same topological level.
///
/// Uses Kahn's algorithm with a BTreeSet queue for deterministic ordering.
fn topological_sort(
    tasks: &[ContextualTaskId],
    resolved: &std::collections::BTreeMap<TaskId, Vec<TaskId>>,
) -> Vec<ContextualTaskId> {
    use std::collections::HashMap;

    if tasks.is_empty() {
        return Vec::new();
    }

    // Build set of TaskIds in the cancel set for quick lookup
    let task_set: HashMap<&TaskId, &ContextualTaskId> = tasks
        .iter()
        .map(|ct| (&ct.task_id, ct))
        .collect();

    // Compute in-degrees (how many prerequisites of each task are also in the set)
    // and reverse adjacency (prerequisite -> its dependents within the set)
    let mut in_degree: HashMap<&TaskId, usize> = HashMap::new();
    let mut dependents: HashMap<&TaskId, Vec<&TaskId>> = HashMap::new();

    for ct in tasks {
        in_degree.entry(&ct.task_id).or_insert(0);
        if let Some(prereqs) = resolved.get(&ct.task_id) {
            for prereq in prereqs {
                if task_set.contains_key(prereq) {
                    *in_degree.entry(&ct.task_id).or_insert(0) += 1;
                    dependents.entry(prereq).or_default().push(&ct.task_id);
                }
            }
        }
    }

    // Kahn's algorithm with BTreeSet for alphabetical tiebreak
    let mut queue: BTreeSet<&TaskId> = BTreeSet::new();
    for (&task_id, &deg) in &in_degree {
        if deg == 0 {
            queue.insert(task_id);
        }
    }

    let mut result = Vec::with_capacity(tasks.len());
    while let Some(task_id) = queue.pop_first() {
        result.push(task_set[task_id].clone());
        if let Some(deps) = dependents.get(task_id) {
            for dep in deps {
                if let Some(deg) = in_degree.get_mut(dep) {
                    *deg -= 1;
                    if *deg == 0 {
                        queue.insert(dep);
                    }
                }
            }
        }
    }

    debug_assert_eq!(result.len(), tasks.len(), "topological_sort: cycle detected — {} tasks not processed", tasks.len() - result.len());

    result
}
