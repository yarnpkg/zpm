use std::sync::Arc;

use super::super::ipc::DaemonResponse;
use super::super::process_registry::ProcessRegistry;
use super::super::scheduler::Scheduler;

pub fn handle_cancel_context(
    context_id: &str,
    scheduler: &Scheduler,
    process_registry: &Arc<ProcessRegistry>,
) -> DaemonResponse {
    // Mark all tasks in the context as failed
    let cancelled_ids = scheduler.cancel_context(context_id);
    let cancelled_count = cancelled_ids.len();

    // Atomically claim and remove all PIDs for this context.
    // This prevents race conditions where a task might complete naturally between
    // checking for its PID and attempting to kill it, which could result in
    // sending signals to a reused PID belonging to a different process.
    let pids = process_registry.take_pids_for_context(context_id);

    #[cfg(unix)]
    {
        for pid in pids {
            // Use killpg to kill the entire process group (since children are spawned with process_group(0))
            let result = unsafe { libc::killpg(pid as i32, libc::SIGTERM) };
            if result != 0 {
                // If killpg fails (e.g., group doesn't exist), try killing the process directly
                let _ = unsafe { libc::kill(pid as i32, libc::SIGTERM) };
            }
        }
    }

    DaemonResponse::ContextCancelled { cancelled_count }
}
