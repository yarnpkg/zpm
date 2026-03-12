use std::collections::{HashMap, HashSet};
use std::sync::RwLock;

/// Registry for tracking all running child process IDs.
/// Used for signal propagation when the daemon exits.
pub struct ProcessRegistry {
    /// All registered PIDs
    pids: RwLock<HashSet<u32>>,
    /// Mapping from task_id string to PID for context-based cancellation
    task_to_pid: RwLock<HashMap<String, u32>>,
}

impl ProcessRegistry {
    pub fn new() -> Self {
        Self {
            pids: RwLock::new(HashSet::new()),
            task_to_pid: RwLock::new(HashMap::new()),
        }
    }

    /// Register a new child process PID with its task ID.
    pub fn register(&self, pid: u32) {
        let mut pids = self.pids.write().expect("process registry lock poisoned");
        pids.insert(pid);
    }

    /// Register a new child process PID with its task ID.
    pub fn register_with_task(&self, pid: u32, task_id: String) {
        let mut pids = self.pids.write().expect("process registry lock poisoned");
        pids.insert(pid);

        let mut task_to_pid = self.task_to_pid.write().expect("process registry lock poisoned");
        task_to_pid.insert(task_id, pid);
    }

    /// Unregister a child process PID (when it exits).
    pub fn unregister(&self, pid: u32) {
        let mut pids = self.pids.write().expect("process registry lock poisoned");
        pids.remove(&pid);
    }

    /// Unregister a child process PID with its task ID.
    pub fn unregister_with_task(&self, pid: u32, task_id: &str) {
        let mut pids = self.pids.write().expect("process registry lock poisoned");
        pids.remove(&pid);

        let mut task_to_pid = self.task_to_pid.write().expect("process registry lock poisoned");
        task_to_pid.remove(task_id);
    }

    /// Get all currently registered PIDs.
    pub fn get_all_pids(&self) -> Vec<u32> {
        let pids = self.pids.read().expect("process registry lock poisoned");
        pids.iter().cloned().collect()
    }

    /// Get PIDs for all tasks in a given context (task IDs ending with @context_id).
    pub fn get_pids_for_context(&self, context_id: &str) -> Vec<u32> {
        let suffix = format!("@{}", context_id);
        let task_to_pid = self.task_to_pid.read().expect("process registry lock poisoned");
        task_to_pid
            .iter()
            .filter(|(task_id, _)| task_id.ends_with(&suffix))
            .map(|(_, &pid)| pid)
            .collect()
    }
}
