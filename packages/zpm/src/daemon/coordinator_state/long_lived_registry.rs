use std::{collections::HashMap, time::SystemTime};

use tokio::task::AbortHandle;
use zpm_tasks::TaskId;

use super::super::scheduler::ContextualTaskId;

#[derive(Debug, Clone)]
pub struct LongLivedEntry {
    pub task_id: TaskId,
    pub contextual_task_id: ContextualTaskId,
    pub warm_up_complete: bool,
    pub started_at: SystemTime,
}

/// Owns long-lived task entries.
/// Only modified by the coordinator event loop — no locks needed.
pub struct LongLivedRegistry {
    entries: HashMap<TaskId, LongLivedEntry>,
    /// Abort handles for pending warm-up timers, keyed by TaskId.
    /// When a task is stopped, the handle is aborted so the stale
    /// timer never fires.
    warmup_handles: HashMap<TaskId, AbortHandle>,
}

impl LongLivedRegistry {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
            warmup_handles: HashMap::new(),
        }
    }

    pub fn get(&self, task_id: &TaskId) -> Option<&LongLivedEntry> {
        self.entries.get(task_id)
    }

    pub fn register(&mut self, task_id: TaskId, contextual_task_id: ContextualTaskId) {
        self.entries.insert(
            task_id.clone(),
            LongLivedEntry {
                task_id,
                contextual_task_id,
                warm_up_complete: false,
                started_at: SystemTime::now(),
            },
        );
    }

    pub fn remove(&mut self, task_id: &TaskId) -> Option<LongLivedEntry> {
        self.cancel_warmup(task_id);
        self.entries.remove(task_id)
    }

    pub fn mark_warm_up_complete(&mut self, task_id: &TaskId) {
        if let Some(entry) = self.entries.get_mut(task_id) {
            entry.warm_up_complete = true;
        }
    }

    /// Store the abort handle for a pending warm-up timer.
    /// Any previous handle for the same task is aborted first.
    pub fn set_warmup_handle(&mut self, task_id: TaskId, handle: AbortHandle) {
        if let Some(old) = self.warmup_handles.insert(task_id, handle) {
            old.abort();
        }
    }

    /// Cancel a pending warm-up timer for a task (no-op if none).
    pub fn cancel_warmup(&mut self, task_id: &TaskId) {
        if let Some(handle) = self.warmup_handles.remove(task_id) {
            handle.abort();
        }
    }

    pub fn list(&self) -> Vec<LongLivedEntry> {
        self.entries.values().cloned().collect()
    }
}
