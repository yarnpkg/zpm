use std::collections::BTreeMap;
use std::sync::LazyLock;

use serde::Serialize;
use zpm_primitives::{Ident, IdentGlob};
use zpm_utils::{
    impl_file_string_from_str, impl_file_string_serialization, DataType, FromFileString,
    ToFileString, ToHumanString,
};

#[derive(thiserror::Error, Clone, Debug)]
pub enum TaskNameError {
    #[error("Invalid task name: {0}")]
    SyntaxError(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TaskName(String);

static TASK_NAME_REGEX: LazyLock<regex::Regex>
    = LazyLock::new(|| regex::Regex::new(r"^[a-zA-Z_][a-zA-Z0-9_-]*$").unwrap());

impl TaskName {
    pub fn new(name: &str) -> Result<Self, TaskNameError> {
        if !TASK_NAME_REGEX.is_match(name) {
            return Err(TaskNameError::SyntaxError(name.to_string()));
        }

        Ok(TaskName(name.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl PartialEq<str> for TaskName {
    fn eq(&self, other: &str) -> bool {
        self.0 == other
    }
}

impl PartialEq<&str> for TaskName {
    fn eq(&self, other: &&str) -> bool {
        self.0 == *other
    }
}

impl std::borrow::Borrow<str> for TaskName {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl FromFileString for TaskName {
    type Error = TaskNameError;

    fn from_file_string(s: &str) -> Result<Self, Self::Error> {
        TaskName::new(s)
    }
}

impl ToFileString for TaskName {
    fn to_file_string(&self) -> String {
        self.0.clone()
    }
}

impl ToHumanString for TaskName {
    fn to_print_string(&self) -> String {
        DataType::Ident.colorize(&self.0)
    }
}

impl_file_string_from_str!(TaskName);
impl_file_string_serialization!(TaskName);

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TaskId {
    pub workspace: Ident,
    pub task_name: TaskName,
}

impl std::fmt::Display for TaskId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.workspace.as_str(), self.task_name.as_str())
    }
}

impl Serialize for TaskId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskFile {
    pub includes: Vec<Include>,
    pub tasks: BTreeMap<TaskName, Task>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Task {
    pub attributes: Vec<Attribute>,
    pub dependencies: Vec<Dependency>,
    pub script: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Attribute {
    pub name: String,
    pub value: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub enum Dependency {
    Local { name: TaskName, parallel: bool },
    External { ident_glob: IdentGlob, task_name: TaskName, parallel: bool },
}

#[derive(Debug, Clone, Serialize)]
pub struct Include {
    pub ident: Ident,
    pub path: Option<String>,
}
