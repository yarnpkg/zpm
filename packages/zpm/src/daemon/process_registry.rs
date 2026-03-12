use std::collections::{HashMap, HashSet};
use std::sync::RwLock;

/// Internal state for ProcessRegistry, protected by a single RwLock
/// to prevent potential deadlocks from multiple lock acquisitions.
struct ProcessRegistryState {
    /// All registered PIDs
    pids: HashSet<u32>,
    /// Mapping from task_id string to PID for context-based cancellation
    task_to_pid: HashMap<String, u32>,
}

/// Registry for tracking all running child process IDs.
/// Used for signal propagation when the daemon exits.
pub struct ProcessRegistry {
    /// Single lock protecting all state to prevent deadlocks.
    /// Previously used separate RwLocks for `pids` and `task_to_pid`,
    /// but acquiring multiple locks in sequence risked deadlock if
    /// any code path acquired them in a different order.
    state: RwLock<ProcessRegistryState>,
}

impl ProcessRegistry {
    pub fn new() -> Self {
        Self {
            state: RwLock::new(ProcessRegistryState {
                pids: HashSet::new(),
                task_to_pid: HashMap::new(),
            }),
        }
    }

    /// Register a new child process PID without a task ID.
    pub fn register(&self, pid: u32) {
        let mut state = self.state.write().expect("process registry lock poisoned");
        state.pids.insert(pid);
    }

    /// Register a new child process PID with its task ID.
    pub fn register_with_task(&self, pid: u32, task_id: String) {
        let mut state = self.state.write().expect("process registry lock poisoned");
        state.pids.insert(pid);
        state.task_to_pid.insert(task_id, pid);
    }

    /// Unregister a child process PID (when it exits).
    pub fn unregister(&self, pid: u32) {
        let mut state = self.state.write().expect("process registry lock poisoned");
        state.pids.remove(&pid);
    }

    /// Unregister a child process PID with its task ID.
    pub fn unregister_with_task(&self, pid: u32, task_id: &str) {
        let mut state = self.state.write().expect("process registry lock poisoned");
        state.pids.remove(&pid);
        state.task_to_pid.remove(task_id);
    }

    /// Get all currently registered PIDs.
    pub fn get_all_pids(&self) -> Vec<u32> {
        let state = self.state.read().expect("process registry lock poisoned");
        state.pids.iter().cloned().collect()
    }

    /// Get PIDs for all tasks in a given context (task IDs ending with @context_id).
    pub fn get_pids_for_context(&self, context_id: &str) -> Vec<u32> {
        let suffix = format!("@{}", context_id);
        let state = self.state.read().expect("process registry lock poisoned");
        state.task_to_pid
            .iter()
            .filter(|(task_id, _)| task_id.ends_with(&suffix))
            .map(|(_, &pid)| pid)
            .collect()
    }

    /// Atomically remove and return a PID for a given task ID.
    /// Returns `Some(pid)` if the task was registered and the PID was removed,
    /// or `None` if the task was not registered (e.g., already completed).
    ///
    /// This prevents race conditions where a task completes naturally between
    /// checking for its PID and attempting to kill it.
    pub fn take_pid_for_task(&self, task_id: &str) -> Option<u32> {
        let mut state = self.state.write().expect("process registry lock poisoned");
        let pid = state.task_to_pid.remove(task_id)?;
        state.pids.remove(&pid);
        Some(pid)
    }

    /// Atomically remove and return all PIDs for tasks in a given context.
    /// Returns a vector of PIDs that were registered and have been removed.
    ///
    /// This prevents race conditions where tasks complete naturally between
    /// checking for their PIDs and attempting to kill them.
    pub fn take_pids_for_context(&self, context_id: &str) -> Vec<u32> {
        let suffix = format!("@{}", context_id);
        let mut state = self.state.write().expect("process registry lock poisoned");

        let task_ids_to_remove: Vec<String> = state
            .task_to_pid
            .keys()
            .filter(|task_id| task_id.ends_with(&suffix))
            .cloned()
            .collect();

        let mut pids = Vec::with_capacity(task_ids_to_remove.len());
        for task_id in task_ids_to_remove {
            if let Some(pid) = state.task_to_pid.remove(&task_id) {
                state.pids.remove(&pid);
                pids.push(pid);
            }
        }

        pids
    }
}
