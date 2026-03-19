use std::collections::HashMap;

// ============================================================================
// Context Registry
// ============================================================================

/// Tracks active contexts and their non-terminal task counts.
///
/// A context is "active" as long as it has at least one non-terminal task.
/// While a context is active, tasks belonging to it must not be evicted
/// from the task graph (otherwise we'd lose the information needed to
/// deduplicate re-pushes of already-completed tasks).
pub struct ContextRegistry {
    /// context_id → number of non-terminal tasks in that context
    active: HashMap<String, usize>,
}

impl ContextRegistry {
    pub fn new() -> Self {
        Self {
            active: HashMap::new(),
        }
    }

    /// Record that a new non-terminal task was added in this context.
    pub fn increment(&mut self, context_id: &str) {
        *self.active.entry(context_id.to_string()).or_insert(0) += 1;
    }

    /// Record that a task in this context reached a terminal state.
    /// Removes the context entry when the count reaches zero.
    pub fn decrement(&mut self, context_id: &str) {
        if let Some(count) = self.active.get_mut(context_id) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                self.active.remove(context_id);
            }
        }
    }

    /// Returns true if the context has at least one non-terminal task.
    pub fn is_active(&self, context_id: &str) -> bool {
        self.active.contains_key(context_id)
    }

    // ========================================================================
    // Statistics
    // ========================================================================

    pub fn active_contexts_count(&self) -> usize {
        self.active.len()
    }
}
