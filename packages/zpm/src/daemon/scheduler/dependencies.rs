use std::collections::{BTreeMap, HashSet};

use zpm_tasks::ResolvedTasks;

use super::state::{ContextualTaskId, PreparedTask};

/// Find tasks that are ready to execute.
/// A task is ready when all its prerequisites are completed (in the same context).
/// For long-lived prerequisites, being "warmed up" counts as ready for dependents.
pub fn find_ready_tasks(
    resolved: &ResolvedTasks,
    completed: &HashSet<ContextualTaskId>,
    failed: &HashSet<ContextualTaskId>,
    script_finished: &HashSet<ContextualTaskId>,
    warm_up_complete: &HashSet<ContextualTaskId>,
    running: &HashSet<ContextualTaskId>,
    targets: &HashSet<ContextualTaskId>,
    prepared: &BTreeMap<ContextualTaskId, PreparedTask>,
) -> Vec<ContextualTaskId> {
    // Collect all contexts that have pending work (including newly added targets)
    let active_contexts: HashSet<&String> = completed
        .iter()
        .chain(failed.iter())
        .chain(script_finished.iter())
        .chain(running.iter())
        .chain(targets.iter())
        .map(|ctx_id| &ctx_id.context_id)
        .collect();

    let mut ready = Vec::new();

    // For each context, check which tasks are ready
    for context_id in active_contexts {
        for (task_id, prerequisites) in &resolved.tasks {
            let ctx_task_id = ContextualTaskId::new(task_id.clone(), context_id.clone());

            // Skip if already completed, failed, finished, or running
            if completed.contains(&ctx_task_id)
                || failed.contains(&ctx_task_id)
                || script_finished.contains(&ctx_task_id)
                || running.contains(&ctx_task_id)
            {
                continue;
            }

            // Check if all prerequisites are ready (in the same context)
            // For regular tasks, "ready" means completed.
            // For long-lived tasks, "ready" means warm-up complete.
            let all_prereqs_ready = prerequisites.iter().all(|prereq| {
                let ctx_prereq
                    = ContextualTaskId::new(prereq.clone(), context_id.clone());

                if failed.contains(&ctx_prereq) {
                    return false;
                }

                if completed.contains(&ctx_prereq) {
                    return true;
                }

                let is_long_lived
                    = prepared
                        .get(&ctx_prereq)
                        .map(|p| p.is_long_lived)
                        .unwrap_or(false);

                if is_long_lived && warm_up_complete.contains(&ctx_prereq) {
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
    resolved: &ResolvedTasks,
    completed: &HashSet<ContextualTaskId>,
    failed: &HashSet<ContextualTaskId>,
    running: &HashSet<ContextualTaskId>,
) -> Vec<ContextualTaskId> {
    // Collect all contexts that have pending work
    let active_contexts: HashSet<&String> = completed
        .iter()
        .chain(failed.iter())
        .chain(running.iter())
        .map(|ctx_id| &ctx_id.context_id)
        .collect();

    let mut to_fail = Vec::new();

    // For each context, check which tasks should fail
    for context_id in active_contexts {
        for (task_id, prerequisites) in &resolved.tasks {
            let ctx_task_id = ContextualTaskId::new(task_id.clone(), context_id.clone());

            // Skip if already completed, failed, or running
            if completed.contains(&ctx_task_id)
                || failed.contains(&ctx_task_id)
                || running.contains(&ctx_task_id)
            {
                continue;
            }

            // Check if any prerequisite failed (in the same context)
            let any_prereq_failed = prerequisites.iter().any(|prereq| {
                let ctx_prereq = ContextualTaskId::new(prereq.clone(), context_id.clone());
                failed.contains(&ctx_prereq)
            });

            if any_prereq_failed {
                to_fail.push(ctx_task_id);
            }
        }
    }

    to_fail
}
