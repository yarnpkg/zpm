use std::collections::HashSet;

use zpm_tasks::ResolvedTasks;

use super::state::ContextualTaskId;

/// Find tasks that are ready to execute.
/// A task is ready when all its prerequisites are completed (in the same context).
pub fn find_ready_tasks(
    resolved: &ResolvedTasks,
    completed: &HashSet<ContextualTaskId>,
    failed: &HashSet<ContextualTaskId>,
    script_finished: &HashSet<ContextualTaskId>,
    running: &HashSet<ContextualTaskId>,
    targets: &HashSet<ContextualTaskId>,
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

            // Check if all prerequisites are completed (in the same context)
            let all_prereqs_completed = prerequisites.iter().all(|prereq| {
                let ctx_prereq = ContextualTaskId::new(prereq.clone(), context_id.clone());
                completed.contains(&ctx_prereq) && !failed.contains(&ctx_prereq)
            });

            if all_prereqs_completed {
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
