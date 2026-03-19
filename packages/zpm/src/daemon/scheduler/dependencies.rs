use std::collections::HashSet;

use super::state::ContextualTaskId;
use crate::daemon::coordinator_state::TaskGraph;

/// Find tasks that are ready to execute.
/// A task is ready when all its prerequisites are completed (in the same context).
/// For long-lived prerequisites, being "warmed up" counts as ready for dependents.
pub fn find_ready_tasks(
    graph: &TaskGraph,
    running: &HashSet<ContextualTaskId>,
) -> Vec<ContextualTaskId> {
    // Collect all contexts that have pending work
    let active_contexts: HashSet<&String> = graph
        .tasks
        .keys()
        .chain(running.iter())
        .map(|ctx_id| &ctx_id.context_id)
        .collect();

    let mut ready = Vec::new();

    // For each context, check which tasks are ready
    for context_id in active_contexts {
        for (task_id, prerequisites) in &graph.resolved.tasks {
            let ctx_task_id = ContextualTaskId::new(task_id.clone(), context_id.clone());

            if !graph.prepared.contains_key(&ctx_task_id) {
                continue;
            }

            // Skip if already completed, failed, finished, or running
            let task_state = graph.get_state(&ctx_task_id);
            if task_state.is_terminal()
                || task_state.is_script_finished()
                || running.contains(&ctx_task_id)
            {
                continue;
            }

            // Check if all prerequisites are ready (in the same context)
            // For regular tasks, "ready" means completed.
            // For long-lived tasks, "ready" means warm-up complete.
            let all_prereqs_ready = prerequisites.iter().all(|prereq| {
                let ctx_prereq = ContextualTaskId::new(prereq.clone(), context_id.clone());

                if graph.is_failed_or_cancelled(&ctx_prereq) {
                    return false;
                }

                if graph.is_completed(&ctx_prereq) {
                    return true;
                }

                let is_long_lived = graph
                    .prepared
                    .get(&ctx_prereq)
                    .map(|p| p.is_long_lived)
                    .unwrap_or(false);

                if is_long_lived && graph.is_warm_up_complete(&ctx_prereq) {
                    return true;
                }

                false
            });

            if all_prereqs_ready {
                ready.push(ctx_task_id);
            }
        }
    }

    ready
}

/// Find tasks that should be marked as failed because a prerequisite failed.
pub fn find_tasks_to_fail(
    graph: &TaskGraph,
    running: &HashSet<ContextualTaskId>,
) -> Vec<ContextualTaskId> {
    // Collect all contexts that have pending work
    let active_contexts: HashSet<&String> = graph
        .tasks
        .keys()
        .chain(running.iter())
        .map(|ctx_id| &ctx_id.context_id)
        .collect();

    let mut to_fail = Vec::new();

    // For each context, check which tasks should fail
    for context_id in active_contexts {
        for (task_id, prerequisites) in &graph.resolved.tasks {
            let ctx_task_id = ContextualTaskId::new(task_id.clone(), context_id.clone());

            if !graph.prepared.contains_key(&ctx_task_id) {
                continue;
            }

            // Skip if already completed, failed, script finished (e.g. waiting for subtasks), or running
            let task_state = graph.get_state(&ctx_task_id);
            if task_state.is_terminal() || task_state.is_script_finished() || running.contains(&ctx_task_id) {
                continue;
            }

            // Check if any prerequisite failed or was cancelled (in the same context)
            let any_prereq_failed = prerequisites.iter().any(|prereq| {
                let ctx_prereq = ContextualTaskId::new(prereq.clone(), context_id.clone());
                graph.is_failed_or_cancelled(&ctx_prereq)
            });

            if any_prereq_failed {
                to_fail.push(ctx_task_id);
            }
        }
    }

    to_fail
}
