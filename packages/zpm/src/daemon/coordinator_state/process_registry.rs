use std::collections::{HashMap, HashSet};
use std::time::Instant;

// ============================================================================
// Spawning Task State
// ============================================================================

#[derive(Debug)]
struct SpawningEntry {
    #[allow(dead_code)]
    spawned_at: Instant,
    pending_cancel: bool,
}

// ============================================================================
// Process Registry
// ============================================================================

/// Owns PID tracking and the spawning state machine.
/// Only modified by the coordinator event loop — no locks needed.
pub struct ProcessRegistry {
    /// All registered PIDs
    pids: HashSet<u32>,
    /// Mapping from task_id string to PID
    task_to_pid: HashMap<String, u32>,
    /// Tasks currently spawning (between spawn() and PID registration)
    spawning: HashMap<String, SpawningEntry>,
}

impl ProcessRegistry {
    pub fn new() -> Self {
        Self {
            pids: HashSet::new(),
            task_to_pid: HashMap::new(),
            spawning: HashMap::new(),
        }
    }

    // ========================================================================
    // PID Operations
    // ========================================================================

    pub fn register_pid(&mut self, pid: u32, task_id: String) {
        self.pids.insert(pid);
        self.task_to_pid.insert(task_id, pid);
    }

    pub fn unregister_pid(&mut self, pid: u32, task_id: &str) {
        self.pids.remove(&pid);
        self.task_to_pid.remove(task_id);
    }

    pub fn get_all_pids(&self) -> Vec<u32> {
        self.pids.iter().cloned().collect()
    }

    pub fn take_pid_for_task(&mut self, task_id: &str) -> Option<u32> {
        let pid = self.task_to_pid.remove(task_id)?;
        self.pids.remove(&pid);
        Some(pid)
    }

    pub fn take_pids_for_context(&mut self, context_id: &str) -> Vec<u32> {
        let suffix = format!("@{}", context_id);
        let task_ids_to_remove: Vec<String> = self
            .task_to_pid
            .keys()
            .filter(|task_id| task_id.ends_with(&suffix))
            .cloned()
            .collect();

        let mut pids = Vec::with_capacity(task_ids_to_remove.len());
        for task_id in task_ids_to_remove {
            if let Some(pid) = self.task_to_pid.remove(&task_id) {
                self.pids.remove(&pid);
                pids.push(pid);
            }
        }

        pids
    }

    // ========================================================================
    // Spawning Operations
    // ========================================================================

    pub fn mark_spawning(&mut self, task_id: String) {
        self.spawning.insert(task_id, SpawningEntry {
            spawned_at: Instant::now(),
            pending_cancel: false,
        });
    }

    pub fn mark_spawning_pending_cancel(&mut self, task_id: &str) -> bool {
        if let Some(entry) = self.spawning.get_mut(task_id) {
            entry.pending_cancel = true;
            true
        } else {
            false
        }
    }

    pub fn take_spawning(&mut self, task_id: &str) -> Option<bool> {
        self.spawning.remove(task_id).map(|e| e.pending_cancel)
    }

    pub fn get_spawning_for_context(&self, context_id: &str) -> Vec<String> {
        let suffix = format!("@{}", context_id);
        self.spawning
            .keys()
            .filter(|id| id.ends_with(&suffix))
            .cloned()
            .collect()
    }
}
