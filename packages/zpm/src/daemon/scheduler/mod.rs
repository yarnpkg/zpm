mod dependencies;
mod state;

use std::collections::HashSet;
use std::sync::RwLock;

use zpm_primitives::Ident;
use zpm_tasks::{TaskId, TaskName};
use zpm_utils::ToFileString;

pub use state::{ContextualTaskId, PreparedTask};

use crate::error::Error;
use crate::project::Project;

use self::state::SchedulerState;

pub struct Scheduler {
    state: RwLock<SchedulerState>,
}

impl Scheduler {
    pub fn new() -> Self {
        Self {
            state: RwLock::new(SchedulerState::new()),
        }
    }

    pub fn add_task(
        &self,
        project: &Project,
        task_name: &str,
        parent_task_id: Option<&str>,
        args: Vec<String>,
        workspace_override: Option<&str>,
        context_id: Option<&str>,
    ) -> Result<(ContextualTaskId, Vec<ContextualTaskId>), Error> {
        let mut state = self.state.write().unwrap();
        state.add_task(project, task_name, parent_task_id, args, workspace_override, context_id)
    }

    pub fn ready_tasks(&self, running: &HashSet<ContextualTaskId>) -> Vec<(ContextualTaskId, Option<PreparedTask>)> {
        let state
            = self.state.read().unwrap();

        let ready_ids
            = dependencies::find_ready_tasks(
                &state.resolved,
                &state.completed,
                &state.failed,
                &state.script_finished,
                &state.warm_up_complete,
                running,
                &state.targets,
                &state.prepared,
            );

        ready_ids
            .into_iter()
            .map(|ctx_task_id| {
                let prepared
                    = state.prepared.get(&ctx_task_id).cloned();

                (ctx_task_id, prepared)
            })
            .collect()
    }

    pub fn tasks_to_fail(&self, running: &HashSet<ContextualTaskId>) -> Vec<ContextualTaskId> {
        let state = self.state.read().unwrap();

        dependencies::find_tasks_to_fail(
            &state.resolved,
            &state.completed,
            &state.failed,
            running,
        )
    }

    pub fn mark_script_finished(&self, task_id: &ContextualTaskId) {
        let mut state = self.state.write().unwrap();
        state.script_finished.insert(task_id.clone());
    }

    pub fn mark_completed(&self, task_id: &ContextualTaskId) {
        let mut state = self.state.write().unwrap();
        state.completed.insert(task_id.clone());
    }

    pub fn mark_failed(&self, task_id: &ContextualTaskId) {
        let mut state = self.state.write().unwrap();
        state.failed.insert(task_id.clone());
        state.completed.insert(task_id.clone());
    }

    pub fn try_complete_task(&self, task_id: &ContextualTaskId) -> bool {
        let mut state = self.state.write().unwrap();
        state.try_complete_task(task_id)
    }

    pub fn find_parents(&self, task_id: &ContextualTaskId) -> Vec<ContextualTaskId> {
        let state = self.state.read().unwrap();
        state
            .subtasks
            .iter()
            .filter(|(_, children)| children.contains(task_id))
            .map(|(parent, _)| parent.clone())
            .collect()
    }

    pub fn all_targets_completed(&self) -> bool {
        let state = self.state.read().unwrap();
        state.all_targets_completed()
    }

    pub fn get_prepared_task(&self, task_id: &ContextualTaskId) -> Option<PreparedTask> {
        let state = self.state.read().unwrap();
        state.prepared.get(task_id).cloned()
    }

    pub fn has_prepared_task(&self, task_id: &ContextualTaskId) -> bool {
        let state = self.state.read().unwrap();
        state.prepared.contains_key(task_id)
    }

    pub fn parse_contextual_task_id(&self, task_id_str: &str) -> Option<ContextualTaskId> {
        let (task_part, context_id)
            = task_id_str.rsplit_once('@')?;

        let (workspace_str, task_name_str)
            = task_part.split_once(':')?;

        let task_name
            = TaskName::new(task_name_str).ok()?;

        let workspace
            = Ident::new(workspace_str);

        Some(ContextualTaskId::new(
            TaskId {
                workspace,
                task_name,
            },
            context_id.to_string(),
        ))
    }

    pub fn is_long_lived(&self, task_id: &ContextualTaskId) -> bool {
        let state
            = self.state.read().unwrap();

        state
            .prepared
            .get(task_id)
            .map(|p| p.is_long_lived)
            .unwrap_or(false)
    }

    pub fn mark_warm_up_complete(&self, task_id: &ContextualTaskId) {
        let mut state
            = self.state.write().unwrap();

        state.warm_up_complete.insert(task_id.clone());
    }
}

/// Format a TaskId (without context) as "workspace:taskname"
pub fn format_task_id(task_id: &TaskId) -> String {
    format!(
        "{}:{}",
        task_id.workspace.to_file_string(),
        task_id.task_name.as_str()
    )
}

/// Format a ContextualTaskId as "workspace:taskname@context"
pub fn format_contextual_task_id(ctx_task_id: &ContextualTaskId) -> String {
    format!(
        "{}:{}@{}",
        ctx_task_id.task_id.workspace.to_file_string(),
        ctx_task_id.task_id.task_name.as_str(),
        ctx_task_id.context_id
    )
}
