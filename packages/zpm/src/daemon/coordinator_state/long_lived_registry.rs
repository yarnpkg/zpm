use std::collections::HashMap;
use std::time::SystemTime;

use zpm_tasks::TaskId;

use super::super::scheduler::ContextualTaskId;

// ============================================================================
// Long-Lived Task State
// ============================================================================

#[derive(Debug, Clone)]
pub struct LongLivedEntry {
    pub task_id: TaskId,
    pub contextual_task_id: ContextualTaskId,
    pub warm_up_complete: bool,
    pub started_at: SystemTime,
}

// ============================================================================
// Long-Lived Registry
// ============================================================================

/// Owns long-lived task entries.
/// Only modified by the coordinator event loop — no locks needed.
pub struct LongLivedRegistry {
    entries: HashMap<TaskId, LongLivedEntry>,
}

impl LongLivedRegistry {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
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
        self.entries.remove(task_id)
    }

    pub fn mark_warm_up_complete(&mut self, task_id: &TaskId) -> bool {
        if let Some(entry) = self.entries.get_mut(task_id) {
            entry.warm_up_complete = true;
            true
        } else {
            false
        }
    }

    pub fn list(&self) -> Vec<LongLivedEntry> {
        self.entries.values().cloned().collect()
    }
}
