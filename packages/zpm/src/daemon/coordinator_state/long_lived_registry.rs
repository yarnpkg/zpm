use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime};

use zpm_tasks::TaskId;

use super::super::scheduler::ContextualTaskId;

// ============================================================================
// Long-Lived Task State
// ============================================================================

#[derive(Debug, Clone)]
pub struct LongLivedEntry {
    pub task_id: TaskId,
    pub contextual_task_id: String,
    pub warm_up_complete: bool,
    #[allow(dead_code)]
    pub process_id: Option<u32>,
    pub started_at: SystemTime,
}

struct LongLivedRegistration {
    entry: LongLivedEntry,
    #[allow(dead_code)]
    claimed_at: Option<Instant>,
}

// ============================================================================
// Warm-up Deadline Tracking
// ============================================================================

#[derive(Debug)]
struct WarmUpDeadline {
    contextual_task_id: ContextualTaskId,
    base_task_id: TaskId,
    deadline: Instant,
}

/// An expired warm-up deadline, returned to the coordinator for processing.
pub struct ExpiredWarmUp {
    pub contextual_task_id: ContextualTaskId,
    pub base_task_id: TaskId,
}

// ============================================================================
// Long-Lived Registry
// ============================================================================

/// Owns long-lived task entries and warm-up deadlines.
/// Only modified by the coordinator event loop — no locks needed.
pub struct LongLivedRegistry {
    /// Long-lived task entries
    entries: HashMap<TaskId, LongLivedRegistration>,
    /// Pending warm-up deadlines
    warm_up_deadlines: Vec<WarmUpDeadline>,
}

impl LongLivedRegistry {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
            warm_up_deadlines: Vec::new(),
        }
    }

    // ========================================================================
    // Entry Operations
    // ========================================================================

    pub fn get(&self, task_id: &TaskId) -> Option<LongLivedEntry> {
        self.entries.get(task_id).map(|r| r.entry.clone())
    }

    pub fn register(&mut self, task_id: TaskId, contextual_task_id: String) {
        self.entries.insert(
            task_id.clone(),
            LongLivedRegistration {
                entry: LongLivedEntry {
                    task_id,
                    contextual_task_id,
                    warm_up_complete: false,
                    process_id: None,
                    started_at: SystemTime::now(),
                },
                claimed_at: None,
            },
        );
    }

    pub fn remove(&mut self, task_id: &TaskId) -> Option<LongLivedEntry> {
        self.entries.remove(task_id).map(|r| r.entry)
    }

    pub fn mark_warm_up_complete(&mut self, task_id: &TaskId) -> bool {
        if let Some(reg) = self.entries.get_mut(task_id) {
            reg.entry.warm_up_complete = true;
            true
        } else {
            false
        }
    }

    pub fn list(&self) -> Vec<LongLivedEntry> {
        self.entries
            .values()
            .map(|r| r.entry.clone())
            .collect()
    }

    // ========================================================================
    // Warm-Up Deadline Operations
    // ========================================================================

    pub fn schedule_warm_up(&mut self, contextual_task_id: ContextualTaskId, base_task_id: TaskId, delay: Duration) {
        self.warm_up_deadlines.push(WarmUpDeadline {
            contextual_task_id,
            base_task_id,
            deadline: Instant::now() + delay,
        });
    }

    /// Drain all expired warm-up deadlines. The coordinator is responsible for
    /// filtering out tasks that have already reached a terminal state.
    pub fn drain_expired_deadlines(&mut self) -> Vec<ExpiredWarmUp> {
        let now = Instant::now();
        let mut expired = Vec::new();
        let mut to_remove = Vec::new();

        for (idx, deadline) in self.warm_up_deadlines.iter().enumerate() {
            if now >= deadline.deadline {
                to_remove.push(idx);
                expired.push(ExpiredWarmUp {
                    contextual_task_id: deadline.contextual_task_id.clone(),
                    base_task_id: deadline.base_task_id.clone(),
                });
            }
        }

        for idx in to_remove.into_iter().rev() {
            self.warm_up_deadlines.swap_remove(idx);
        }

        expired
    }
}
