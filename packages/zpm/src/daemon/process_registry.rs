use std::collections::HashSet;
use std::sync::RwLock;

/// Registry for tracking all running child process IDs.
/// Used for signal propagation when the daemon exits.
pub struct ProcessRegistry {
    inner: RwLock<HashSet<u32>>,
}

impl ProcessRegistry {
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(HashSet::new()),
        }
    }

    /// Register a new child process PID.
    pub fn register(&self, pid: u32) {
        let mut inner = self.inner.write().expect("process registry lock poisoned");
        inner.insert(pid);
    }

    /// Unregister a child process PID (when it exits).
    pub fn unregister(&self, pid: u32) {
        let mut inner = self.inner.write().expect("process registry lock poisoned");
        inner.remove(&pid);
    }

    /// Get all currently registered PIDs.
    pub fn get_all_pids(&self) -> Vec<u32> {
        let inner = self.inner.read().expect("process registry lock poisoned");
        inner.iter().cloned().collect()
    }
}
