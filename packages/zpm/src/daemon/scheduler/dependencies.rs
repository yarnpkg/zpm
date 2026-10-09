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
        // Contexts may carry a concurrency limit (`yarn tasks run -j`); tasks
        // without a script don't spawn a process and aren't counted.
        let mut available_slots: Option<usize>
            = graph.concurrency_limits.get(context_id).map(|limit| {
                let running_in_context
                    = running.iter()
                        .filter(|id| &id.context_id == context_id)
                        .count();

                limit.saturating_sub(running_in_context)
            });

        for ctx_task_id in graph.prepared.keys().filter(|id| &id.context_id == context_id) {
            let Some(prerequisites) = graph.prerequisites_of(&ctx_task_id.task_id, context_id) else {
                continue;
            };

            let ctx_task_id = ctx_task_id.clone();

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

            if !all_prereqs_ready {
                continue;
            }

            if let Some(slots) = available_slots.as_mut() {
                let spawns_process
                    = graph.prepared.get(&ctx_task_id)
                        .map_or(false, |prepared| !prepared.script.is_empty());

                if spawns_process {
                    if *slots == 0 {
                        continue;
                    }

                    *slots -= 1;
                }
            }

            ready.push(ctx_task_id);
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
        for ctx_task_id in graph.prepared.keys().filter(|id| &id.context_id == context_id) {
            let Some(prerequisites) = graph.prerequisites_of(&ctx_task_id.task_id, context_id) else {
                continue;
            };

            let ctx_task_id = ctx_task_id.clone();

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
