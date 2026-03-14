// ============================================================================
// Coordinator State (Composed)
//
// This module consolidates all mutable daemon state into sub-structs
// that are owned exclusively by the coordinator loop. No Arc<RwLock> wrappers —
// the coordinator is the single owner, making race conditions structurally
// impossible.
//
// Each sub-struct has a focused API surface. Cross-domain operations are
// orchestrated by the coordinator loop, keeping them explicit and reviewable.
// ============================================================================

mod long_lived_registry;
mod output_buffer;
mod process_registry;
mod subscription_manager;
mod task_graph;

pub use long_lived_registry::LongLivedRegistry;
pub use output_buffer::OutputBuffer;
pub use process_registry::ProcessRegistry;
pub use subscription_manager::{SubscriptionId, SubscriptionManager};
pub use task_graph::{
    format_contextual_task_id, parse_base_task_id, parse_contextual_task_id_simple,
    TaskGraph,
};

// Re-export scheduler types that are used across the coordinator
pub use super::scheduler::{ContextualTaskId, PreparedTask};

/// All mutable daemon state, composed from focused sub-structs.
/// Only modified by the coordinator event loop — no locks needed.
pub struct CoordinatorState {
    pub graph: TaskGraph,
    pub processes: ProcessRegistry,
    pub long_lived: LongLivedRegistry,
    pub subscriptions: SubscriptionManager,
    pub output: OutputBuffer,
}

impl CoordinatorState {
    pub fn new(output_buffer_max_lines: usize, max_closed_tasks: usize) -> Self {
        Self {
            graph: TaskGraph::new(),
            processes: ProcessRegistry::new(),
            long_lived: LongLivedRegistry::new(),
            subscriptions: SubscriptionManager::new(),
            output: OutputBuffer::new(output_buffer_max_lines, max_closed_tasks),
        }
    }
}
