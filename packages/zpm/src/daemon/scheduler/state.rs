use std::collections::BTreeMap;

use zpm_tasks::{TaskId, TaskIdError};
use zpm_utils::{impl_file_string_from_str, impl_file_string_serialization, DataType, FromFileString, Path, ToFileString, ToHumanString};

#[derive(thiserror::Error, Clone, Debug)]
pub enum ContextualTaskIdError {
    #[error("Invalid contextual task id format (expected 'workspace:task@context'): {0}")]
    SyntaxError(String),
    #[error("Invalid task id in contextual task id: {0}")]
    InvalidTaskId(#[from] TaskIdError),
}

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

impl FromFileString for ContextualTaskId {
    type Error = ContextualTaskIdError;

    fn from_file_string(s: &str) -> Result<Self, Self::Error> {
        let (task_part, context_id) = s
            .rsplit_once('@')
            .ok_or_else(|| ContextualTaskIdError::SyntaxError(s.to_string()))?;

        let task_id = TaskId::from_file_string(task_part)?;

        Ok(ContextualTaskId {
            task_id,
            context_id: context_id.to_string(),
        })
    }
}

impl ToFileString for ContextualTaskId {
    fn to_file_string(&self) -> String {
        format!("{}@{}", self.task_id.to_file_string(), self.context_id)
    }
}

impl ToHumanString for ContextualTaskId {
    fn to_print_string(&self) -> String {
        format!("{}{}{}",
            self.task_id.to_print_string(),
            DataType::Task.colorize("@"),
            self.context_id,
        )
    }
}

impl_file_string_from_str!(ContextualTaskId);
impl_file_string_serialization!(ContextualTaskId);

#[derive(Debug, Clone)]
pub struct PreparedTask {
    pub script: String,
    pub cwd: Path,
    pub env: BTreeMap<String, String>,
    pub prefix: String,
    pub args: Vec<String>,
    pub is_long_lived: bool,
}
