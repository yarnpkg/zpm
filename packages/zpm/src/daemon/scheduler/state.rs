use std::collections::BTreeMap;

use zpm_tasks::TaskId;
use zpm_utils::Path;

/// A task ID scoped to a specific execution context.
/// Same TaskId can exist in multiple contexts and run in parallel.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ContextualTaskId {
    pub task_id: TaskId,
    pub context_id: String,
}

impl ContextualTaskId {
    pub fn new(task_id: TaskId, context_id: String) -> Self {
        Self { task_id, context_id }
    }
}

#[derive(Debug, Clone)]
pub struct PreparedTask {
    pub script: String,
    pub cwd: Path,
    pub env: BTreeMap<String, String>,
    pub prefix: String,
    pub args: Vec<String>,
    pub is_long_lived: bool,
}
