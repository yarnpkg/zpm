use zpm_primitives::Ident;

use crate::ast::{TaskId, TaskName};

#[derive(thiserror::Error, Debug, Clone)]
pub enum Error {
    #[error("Parse error at line {line}: {message}")]
    ParseError { line: usize, message: String },

    #[error("Invalid attribute syntax: {0}")]
    InvalidAttribute(String),

    #[error("Invalid task header: {0}")]
    InvalidTaskHeader(String),

    #[error("Invalid dependency: {0}")]
    InvalidDependency(String),

    #[error("Invalid ident glob: {0}")]
    InvalidIdentGlob(String),

    #[error("Unexpected indented line outside of task at line {0}")]
    UnexpectedIndent(usize),

    #[error("Orphaned attributes at end of file (no task follows)")]
    OrphanedAttributes,

    #[error("Duplicate dependency '{name}' in task at line {line}")]
    DuplicateDependency { line: usize, name: String },

    #[error("Workspace not found: {}", .0.as_str())]
    WorkspaceNotFound(Ident),

    #[error("Task '{}' not found in workspace '{}'", task_name.as_str(), workspace.as_str())]
    TaskNotFound { workspace: Ident, task_name: TaskName },

    #[error("Cycle detected: {}", format_cycle(.0))]
    CycleDetected(Vec<TaskId>),

    #[error("Cannot include '{}' from '{}': not listed as a dependency", include_ident.as_str(), workspace.as_str())]
    IncludeNotDependency { workspace: Ident, include_ident: Ident },

    #[error("Cannot load included taskfile '{}' from workspace '{}'", path, workspace.as_str())]
    IncludeLoadError { workspace: Ident, path: String },
}

fn format_cycle(cycle: &[TaskId]) -> String {
    let mut parts: Vec<String>
        = cycle.iter().map(|t| t.to_string()).collect();

    if let Some(first) = cycle.first() {
        parts.push(first.to_string());
    }

    parts.join(" -> ")
}
