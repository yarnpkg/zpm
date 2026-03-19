use std::{collections::{HashMap, HashSet}, time::Instant};

use super::super::scheduler::ContextualTaskId;

#[derive(Debug)]
struct SpawningEntry {
    #[allow(dead_code)]
    spawned_at: Instant,
    pending_cancel: bool,
}

/// Owns PID tracking and the spawning state machine.
/// Only modified by the coordinator event loop — no locks needed.
pub struct ProcessRegistry {
    /// All registered PIDs
    pids: HashSet<u32>,
    /// Mapping from task to PID
    task_to_pid: HashMap<ContextualTaskId, u32>,
    /// Tasks currently spawning (between spawn() and PID registration)
    spawning: HashMap<ContextualTaskId, SpawningEntry>,
}

impl ProcessRegistry {
    pub fn new() -> Self {
        Self {
            pids: HashSet::new(),
            task_to_pid: HashMap::new(),
            spawning: HashMap::new(),
        }
    }

    pub fn register_pid(&mut self, pid: u32, task_id: ContextualTaskId) {
        self.pids.insert(pid);
        self.task_to_pid.insert(task_id, pid);
    }

    pub fn unregister_pid(&mut self, pid: u32, task_id: &ContextualTaskId) {
        self.pids.remove(&pid);
        self.task_to_pid.remove(task_id);
    }

    pub fn get_all_pids(&self) -> Vec<u32> {
        self.pids.iter().cloned().collect()
    }

    pub fn get_pid_for_task(&self, task_id: &ContextualTaskId) -> Option<u32> {
        self.task_to_pid.get(task_id).copied()
    }

    pub fn take_pid_for_task(&mut self, task_id: &ContextualTaskId) -> Option<u32> {
        let pid = self.task_to_pid.remove(task_id)?;
        self.pids.remove(&pid);
        Some(pid)
    }

    pub fn take_pids_for_context(&mut self, context_id: &str) -> Vec<u32> {
        let task_ids_to_remove: Vec<ContextualTaskId> = self
            .task_to_pid
            .keys()
            .filter(|ctx| ctx.context_id == context_id)
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

    pub fn mark_spawning(&mut self, task_id: ContextualTaskId) {
        self.spawning.insert(task_id, SpawningEntry {
            spawned_at: Instant::now(),
            pending_cancel: false,
        });
    }

    pub fn mark_spawning_pending_cancel(&mut self, task_id: &ContextualTaskId) -> bool {
        if let Some(entry) = self.spawning.get_mut(task_id) {
            entry.pending_cancel = true;
            true
        } else {
            false
        }
    }

    pub fn take_spawning(&mut self, task_id: &ContextualTaskId) -> Option<bool> {
        self.spawning.remove(task_id).map(|e| e.pending_cancel)
    }

    pub fn get_spawning_for_context(&self, context_id: &str) -> Vec<ContextualTaskId> {
        self.spawning
            .keys()
            .filter(|ctx| ctx.context_id == context_id)
            .cloned()
            .collect()
    }
}
