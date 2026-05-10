use std::collections::{BTreeMap, HashMap, HashSet};

use zpm_primitives::Ident;
use zpm_tasks::{ResolvedTasks, TaskId, TaskName};
use zpm_utils::{DataType, FromFileString, ToFileString};

use super::{
    super::{
        presentation::prefix_colors,
        scheduler::{ContextualTaskId, PreparedTask},
    },
    context_registry::ContextRegistry,
};
use zpm_utils::Path;

use crate::{
    error::Error,
    project::Project,
};

pub const LONG_LIVED_ATTRIBUTE: &str = "long-lived";

/// Core task execution state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    /// Task is prepared but not yet running
    Pending,
    /// Script has exited, waiting for subtasks to complete
    WaitingForSubtasks { exit_code: i32 },
    /// Task completed successfully
    Completed,
    /// Task failed (script failure or process-level failure)
    Failed,
    /// Task was cancelled because a dependency failed
    Cancelled,
}

impl TaskState {
    pub fn is_terminal(&self) -> bool {
        matches!(self, TaskState::Completed | TaskState::Failed | TaskState::Cancelled)
    }

    pub fn is_script_finished(&self) -> bool {
        matches!(
            self,
            TaskState::WaitingForSubtasks { .. } | TaskState::Completed | TaskState::Failed | TaskState::Cancelled
        )
    }
}

/// Task information including state and metadata
#[derive(Debug, Clone)]
pub struct TaskInfo {
    /// Current execution state
    pub state: TaskState,
    /// Whether this task was explicitly requested (vs. being a dependency)
    pub is_target: bool,
    /// For long-lived tasks: has the warm-up period completed?
    pub warm_up_complete: bool,
}

impl Default for TaskInfo {
    fn default() -> Self {
        Self {
            state: TaskState::Pending,
            is_target: false,
            warm_up_complete: false,
        }
    }
}

/// Owns the dependency/scheduling data and the task state machine.
/// Only modified by the coordinator event loop — no locks needed.
pub struct TaskGraph {
    /// Shared task definitions (keyed by TaskId, not context-specific)
    pub resolved: ResolvedTasks,
    /// Unified task state tracking
    pub tasks: HashMap<ContextualTaskId, TaskInfo>,
    /// Parent-child subtask relationships
    pub subtasks: HashMap<ContextualTaskId, HashSet<ContextualTaskId>>,
    /// Reverse index: child → set of parents (for O(1) parent lookup)
    pub parents: HashMap<ContextualTaskId, HashSet<ContextualTaskId>>,
    /// Prepared task execution info
    pub prepared: BTreeMap<ContextualTaskId, PreparedTask>,
}

impl TaskGraph {
    pub fn new() -> Self {
        Self {
            resolved: ResolvedTasks {
                tasks: BTreeMap::new(),
                task_files: BTreeMap::new(),
            },
            tasks: HashMap::new(),
            subtasks: HashMap::new(),
            parents: HashMap::new(),
            prepared: BTreeMap::new(),
        }
    }

    pub fn add_task(
        &mut self,
        project: &Project,
        task_name: &str,
        parent_task_id: Option<&str>,
        args: Vec<String>,
        workspace_override: Option<&str>,
        context_id: Option<&str>,
        context_registry: &mut ContextRegistry,
    ) -> Result<(ContextualTaskId, Vec<ContextualTaskId>, Vec<Path>), Error> {
        let task_name = TaskName::new(task_name)
            .map_err(|_| Error::TaskNameParseError(task_name.to_string()))?;

        let workspace = if let Some(ws_name) = workspace_override {
            let ident = Ident::new(ws_name);
            project.workspace_by_ident(&ident)?
        } else {
            project.active_workspace()?
        };

        let task_id = TaskId {
            workspace: workspace.name.clone(),
            task_name,
        };

        let ctx_id = if let Some(ctx) = context_id {
            ctx.to_string()
        } else if let Some(parent_str) = parent_task_id {
            ContextualTaskId::from_file_string(parent_str)
                .map(|id| id.context_id)
                .map_err(|_| Error::MissingContextId)?
        } else {
            return Err(Error::MissingContextId);
        };

        let ctx_task_id = ContextualTaskId::new(task_id.clone(), ctx_id.clone());

        // If the task already has an entry in this context (any state,
        // including terminal), skip re-adding. This prevents duplicate
        // pushes of already-completed tasks within the same context.
        //
        // For long-lived task restarts, the coordinator must call
        // clear_task_state before add_task to remove the stale entry.
        if self.tasks.contains_key(&ctx_task_id) {
            // Still register the parent-child link even for duplicate pushes
            if let Some(parent_str) = parent_task_id {
                if let Some(parent_ctx_id) = self.parse_contextual_task_id(project, parent_str) {
                    self.subtasks
                        .entry(parent_ctx_id.clone())
                        .or_default()
                        .insert(ctx_task_id.clone());
                    self.parents
                        .entry(ctx_task_id.clone())
                        .or_default()
                        .insert(parent_ctx_id);
                }
            }
            return Ok((ctx_task_id, vec![], vec![]));
        }

        let resolve_result = project.resolve_task(&task_id)?;
        let new_resolved = resolve_result.resolved;
        let source_files = resolve_result.source_files;

        let mut resolved_ctx_task_ids: Vec<ContextualTaskId> = Vec::new();

        for (tid, prereqs) in new_resolved.tasks {
            let ctx_tid = ContextualTaskId::new(tid.clone(), ctx_id.clone());
            self.clear_task_state(&ctx_tid);
            resolved_ctx_task_ids.push(ctx_tid);
            self.resolved.tasks.entry(tid).or_insert(prereqs);
        }

        // Register the parent-child link AFTER clear_task_state calls,
        // which would otherwise wipe the parent entry we just added.
        if let Some(parent_str) = parent_task_id {
            if let Some(parent_ctx_id) = self.parse_contextual_task_id(project, parent_str) {
                self.subtasks
                    .entry(parent_ctx_id.clone())
                    .or_default()
                    .insert(ctx_task_id.clone());
                self.parents
                    .entry(ctx_task_id.clone())
                    .or_default()
                    .insert(parent_ctx_id);
            }
        }

        for (ident, tf) in new_resolved.task_files {
            self.resolved.task_files.entry(ident).or_insert(tf);
        }

        self.set_as_target(&ctx_task_id);
        self.prepare_specific_tasks(project, &resolved_ctx_task_ids, context_registry)?;

        if !args.is_empty() {
            if let Some(task) = self.prepared.get_mut(&ctx_task_id) {
                task.args = args;
            }
        }

        Ok((ctx_task_id, resolved_ctx_task_ids, source_files))
    }

    /// Prepare only the specific tasks that were resolved for this context.
    fn prepare_specific_tasks(
        &mut self,
        project: &Project,
        tasks_to_prepare: &[ContextualTaskId],
        context_registry: &mut ContextRegistry,
    ) -> Result<usize, Error> {
        let colors: Vec<&DataType> = prefix_colors().take(5).collect();
        let mut color_index = self.prepared.len();
        let mut new_count = 0;

        for ctx_task_id in tasks_to_prepare {
            if self.prepared.contains_key(ctx_task_id) {
                continue;
            }

            let task_id = &ctx_task_id.task_id;

            let Some(task_file) = self.resolved.task_files.get(&task_id.workspace) else {
                continue;
            };

            let Some(task) = task_file.tasks.get(task_id.task_name.as_str()) else {
                continue;
            };

            let Ok(workspace) = project.workspace_by_ident(&task_id.workspace) else {
                continue;
            };

            let script = task.script.join("\n");
            let mut env = BTreeMap::new();

            env.insert(
                "npm_lifecycle_event".to_string(),
                task_id.task_name.as_str().to_string(),
            );

            let color = colors[color_index % colors.len()];
            color_index += 1;

            let prefix = color.colorize(&format!(
                "[{}:{}]: ",
                task_id.workspace.to_file_string(),
                task_id.task_name.as_str()
            ));

            let is_long_lived = task.attributes.iter().any(|attr| attr.name == LONG_LIVED_ATTRIBUTE);

            self.prepared.insert(
                ctx_task_id.clone(),
                PreparedTask {
                    script,
                    cwd: workspace.path.clone(),
                    env,
                    prefix,
                    args: vec![],
                    is_long_lived,
                },
            );

            context_registry.increment(&ctx_task_id.context_id);
            new_count += 1;
        }

        Ok(new_count)
    }

    pub fn clear_task_state(&mut self, task_id: &ContextualTaskId) {
        self.tasks.remove(task_id);
        // Remove forward subtask links and their reverse parent entries
        if let Some(children) = self.subtasks.remove(task_id) {
            for child in &children {
                if let Some(parents) = self.parents.get_mut(child) {
                    parents.remove(task_id);
                }
            }
        }
        self.parents.remove(task_id);
    }

    pub fn get_state(&self, task_id: &ContextualTaskId) -> TaskState {
        self.tasks
            .get(task_id)
            .map(|info| info.state)
            .unwrap_or(TaskState::Pending)
    }

    pub fn is_completed(&self, task_id: &ContextualTaskId) -> bool {
        matches!(self.get_state(task_id), TaskState::Completed)
    }

    #[allow(dead_code)]
    pub fn is_failed(&self, task_id: &ContextualTaskId) -> bool {
        matches!(self.get_state(task_id), TaskState::Failed)
    }

    #[allow(dead_code)]
    pub fn is_cancelled(&self, task_id: &ContextualTaskId) -> bool {
        matches!(self.get_state(task_id), TaskState::Cancelled)
    }

    pub fn is_failed_or_cancelled(&self, task_id: &ContextualTaskId) -> bool {
        matches!(
            self.get_state(task_id),
            TaskState::Failed | TaskState::Cancelled
        )
    }

    pub fn is_terminal(&self, task_id: &ContextualTaskId) -> bool {
        self.get_state(task_id).is_terminal()
    }

    pub fn is_script_finished(&self, task_id: &ContextualTaskId) -> bool {
        self.get_state(task_id).is_script_finished()
    }

    pub fn is_warm_up_complete(&self, task_id: &ContextualTaskId) -> bool {
        self.tasks
            .get(task_id)
            .map(|info| info.warm_up_complete)
            .unwrap_or(false)
    }

    #[allow(dead_code)]
    pub fn is_target(&self, task_id: &ContextualTaskId) -> bool {
        self.tasks
            .get(task_id)
            .map(|info| info.is_target)
            .unwrap_or(false)
    }

    pub fn get_waiting_exit_code(&self, task_id: &ContextualTaskId) -> Option<i32> {
        match self.tasks.get(task_id)?.state {
            TaskState::WaitingForSubtasks { exit_code } => Some(exit_code),
            _ => None,
        }
    }

    pub fn is_task_fully_completed(&self, task_id: &ContextualTaskId) -> bool {
        if !self.is_script_finished(task_id) {
            return false;
        }

        if let Some(task_subtasks) = self.subtasks.get(task_id) {
            task_subtasks.iter().all(|s| self.is_completed(s) && !self.is_failed_or_cancelled(s))
        } else {
            true
        }
    }

    pub fn is_long_lived(&self, task_id: &ContextualTaskId) -> bool {
        self.prepared
            .get(task_id)
            .map(|p| p.is_long_lived)
            .unwrap_or(false)
    }

    pub fn has_failed_subtask(&self, task_id: &ContextualTaskId) -> bool {
        if let Some(subtasks) = self.subtasks.get(task_id) {
            subtasks.iter().any(|s| self.is_failed_or_cancelled(s))
        } else {
            false
        }
    }

    pub fn find_parents(&self, task_id: &ContextualTaskId) -> Vec<ContextualTaskId> {
        self.parents
            .get(task_id)
            .map(|parents| parents.iter().cloned().collect())
            .unwrap_or_default()
    }

    pub fn should_spawn_task(&self, task_id: &ContextualTaskId) -> bool {
        !self.is_terminal(task_id)
    }

    fn ensure_task_info(&mut self, task_id: &ContextualTaskId) -> &mut TaskInfo {
        self.tasks.entry(task_id.clone()).or_default()
    }

    pub fn set_as_target(&mut self, task_id: &ContextualTaskId) {
        self.ensure_task_info(task_id).is_target = true;
    }

    pub fn mark_script_finished_with_code(&mut self, task_id: &ContextualTaskId, exit_code: i32) {
        self.ensure_task_info(task_id).state = TaskState::WaitingForSubtasks { exit_code };
    }

    pub fn try_complete_task(&mut self, task_id: &ContextualTaskId) -> bool {
        if self.is_task_fully_completed(task_id) {
            self.ensure_task_info(task_id).state = TaskState::Completed;
            true
        } else {
            false
        }
    }

    pub fn mark_completed(&mut self, task_id: &ContextualTaskId) {
        self.ensure_task_info(task_id).state = TaskState::Completed;
    }

    pub fn mark_failed(&mut self, task_id: &ContextualTaskId) {
        self.ensure_task_info(task_id).state = TaskState::Failed;
    }

    pub fn mark_cancelled(&mut self, task_id: &ContextualTaskId) {
        self.ensure_task_info(task_id).state = TaskState::Cancelled;
    }

    pub fn mark_warm_up_complete(&mut self, task_id: &ContextualTaskId) {
        self.ensure_task_info(task_id).warm_up_complete = true;
    }

    /// Remove all state for a closed task (used by output buffer eviction).
    pub fn evict_closed_task(&mut self, task_id: &ContextualTaskId) {
        self.tasks.remove(task_id);
        self.prepared.remove(task_id);
        if let Some(children) = self.subtasks.remove(task_id) {
            for child in &children {
                if let Some(parents) = self.parents.get_mut(child) {
                    parents.remove(task_id);
                }
            }
        }
        self.parents.remove(task_id);
    }

    fn parse_contextual_task_id(&self, project: &Project, task_id_str: &str) -> Option<ContextualTaskId> {
        let ctx_task_id = ContextualTaskId::from_file_string(task_id_str).ok()?;

        // Validate that the workspace exists in the project
        let workspace = project.workspace_by_ident(&ctx_task_id.task_id.workspace).ok()?;

        Some(ContextualTaskId::new(
            TaskId {
                workspace: workspace.name.clone(),
                task_name: ctx_task_id.task_id.task_name,
            },
            ctx_task_id.context_id,
        ))
    }

    pub fn tasks_count(&self) -> usize {
        self.tasks.len()
    }

    pub fn prepared_count(&self) -> usize {
        self.prepared.len()
    }

    pub fn subtasks_count(&self) -> usize {
        self.subtasks.len()
    }
}

