use std::collections::{HashMap, HashSet};

use zpm_tasks::TaskId;

use super::state::ContextualTaskId;
use crate::daemon::coordinator_state::{
    TaskGraph,
    TaskState,
};

/// Borrowed `(task, context)` key, letting the scheduler look up task states
/// without allocating a `ContextualTaskId` for every prerequisite check.
type TaskKey<'a> = (&'a TaskId, &'a str);

/// Find tasks that are ready to execute.
/// A task is ready when all its prerequisites are completed (in the same context).
/// For long-lived prerequisites, being "warmed up" counts as ready for dependents.
///
/// This runs after every state-changing coordinator command, so it must stay
/// cheap on large graphs: the set of satisfied tasks is computed once per
/// pass, and prerequisites are checked against it without allocating.
pub fn find_ready_tasks(
    graph: &TaskGraph,
    running: &HashSet<ContextualTaskId>,
) -> Vec<ContextualTaskId> {
    let mut ready = Vec::new();

    // Tasks whose dependents may start: completed ones, and long-lived ones
    // once warmed up.
    let mut satisfied: HashSet<TaskKey<'_>>
        = HashSet::new();

    for (ctx_task_id, info) in &graph.tasks {
        let is_satisfied = match info.state {
            TaskState::Completed => true,
            TaskState::Failed | TaskState::Cancelled => false,
            _ => info.warm_up_complete && graph.is_long_lived(ctx_task_id),
        };

        if is_satisfied {
            satisfied.insert((&ctx_task_id.task_id, ctx_task_id.context_id.as_str()));
        }
    }

    // Contexts may carry a concurrency limit (`yarn tasks run -j`); tasks
    // without a script don't spawn a process and aren't counted.
    let mut available_slots: HashMap<&String, usize>
        = HashMap::new();

    for (context_id, limit) in &graph.concurrency_limits {
        let running_in_context
            = running.iter()
                .filter(|id| &id.context_id == context_id)
                .count();

        available_slots.insert(context_id, limit.saturating_sub(running_in_context));
    }

    for (ctx_task_id, prepared) in &graph.prepared {
        // Skip if already completed, failed, finished, or running
        let task_state = graph.get_state(ctx_task_id);
        if task_state.is_terminal()
            || task_state.is_script_finished()
            || running.contains(ctx_task_id)
        {
            continue;
        }

        let spawns_process
            = !prepared.script.is_empty();

        if spawns_process && available_slots.get(&ctx_task_id.context_id) == Some(&0) {
            continue;
        }

        let Some(prerequisites) = graph.prerequisites_of(&ctx_task_id.task_id, &ctx_task_id.context_id) else {
            continue;
        };

        let context_id
            = ctx_task_id.context_id.as_str();

        let all_prereqs_ready = prerequisites.iter()
            .all(|prereq| satisfied.contains(&(prereq, context_id)));

        if !all_prereqs_ready {
            continue;
        }

        if spawns_process {
            if let Some(slots) = available_slots.get_mut(&ctx_task_id.context_id) {
                *slots -= 1;
            }
        }

        ready.push(ctx_task_id.clone());
    }

    ready
}

/// Find tasks that should be marked as failed because a prerequisite failed.
pub fn find_tasks_to_fail(
    graph: &TaskGraph,
    running: &HashSet<ContextualTaskId>,
) -> Vec<ContextualTaskId> {
    let failed: HashSet<TaskKey<'_>>
        = graph.tasks.iter()
            .filter(|(_, info)| matches!(info.state, TaskState::Failed | TaskState::Cancelled))
            .map(|(ctx_task_id, _)| (&ctx_task_id.task_id, ctx_task_id.context_id.as_str()))
            .collect();

    // Happy path: nothing failed, nothing to propagate
    if failed.is_empty() {
        return Vec::new();
    }

    let mut to_fail = Vec::new();

    for ctx_task_id in graph.prepared.keys() {
        // Skip if already completed, failed, script finished (e.g. waiting for subtasks), or running
        let task_state = graph.get_state(ctx_task_id);
        if task_state.is_terminal() || task_state.is_script_finished() || running.contains(ctx_task_id) {
            continue;
        }

        let Some(prerequisites) = graph.prerequisites_of(&ctx_task_id.task_id, &ctx_task_id.context_id) else {
            continue;
        };

        let context_id
            = ctx_task_id.context_id.as_str();

        // Check if any prerequisite failed or was cancelled (in the same context)
        let any_prereq_failed = prerequisites.iter()
            .any(|prereq| failed.contains(&(prereq, context_id)));

        if any_prereq_failed {
            to_fail.push(ctx_task_id.clone());
        }
    }

    to_fail
}
