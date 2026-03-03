use std::collections::{BTreeMap, HashMap, HashSet};

use zpm_primitives::Ident;
use zpm_tasks::{ResolvedTasks, TaskId, TaskName};
use zpm_utils::{DataType, Path, ToFileString};

use super::super::presentation::prefix_colors;
use crate::error::Error;
use crate::project::Project;

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

pub struct SchedulerState {
    /// Shared task definitions (keyed by TaskId, not context-specific)
    pub resolved: ResolvedTasks,
    /// Tasks that have been directly requested (context-specific)
    pub targets: HashSet<ContextualTaskId>,
    /// Tasks that have fully completed (context-specific)
    pub completed: HashSet<ContextualTaskId>,
    /// Tasks that have failed (context-specific)
    pub failed: HashSet<ContextualTaskId>,
    /// Tasks whose scripts have finished (context-specific)
    pub script_finished: HashSet<ContextualTaskId>,
    /// Long-lived tasks that have completed their warm-up period (context-specific)
    pub warm_up_complete: HashSet<ContextualTaskId>,
    /// Parent-child subtask relationships (context-specific)
    pub subtasks: HashMap<ContextualTaskId, HashSet<ContextualTaskId>>,
    /// Prepared task execution info (context-specific)
    pub prepared: BTreeMap<ContextualTaskId, PreparedTask>,
}

impl SchedulerState {
    pub fn new() -> Self {
        Self {
            resolved: ResolvedTasks {
                tasks: BTreeMap::new(),
                task_files: BTreeMap::new(),
            },
            targets: HashSet::new(),
            completed: HashSet::new(),
            failed: HashSet::new(),
            script_finished: HashSet::new(),
            warm_up_complete: HashSet::new(),
            subtasks: HashMap::new(),
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
    ) -> Result<(ContextualTaskId, Vec<ContextualTaskId>), Error> {
        let task_name
            = TaskName::new(task_name)
                .map_err(|_| Error::TaskNameParseError(task_name.to_string()))?;

        let workspace
            = if let Some(ws_name) = workspace_override {
                let ident
                    = Ident::new(ws_name);

                project.workspace_by_ident(&ident)?
            } else {
                project.active_workspace()?
            };

        let task_id
            = TaskId {
                workspace: workspace.name.clone(),
                task_name,
            };

        let ctx_id
            = if let Some(ctx) = context_id {
                ctx.to_string()
            } else if let Some(parent_str) = parent_task_id {
                self.parse_context_id(parent_str)
                    .ok_or_else(|| Error::MissingContextId)?
            } else {
                return Err(Error::MissingContextId);
            };

        let ctx_task_id
            = ContextualTaskId::new(task_id.clone(), ctx_id.clone());

        if let Some(parent_str) = parent_task_id {
            if let Some(parent_ctx_id) = self.parse_contextual_task_id(project, parent_str) {
                self.subtasks
                    .entry(parent_ctx_id)
                    .or_default()
                    .insert(ctx_task_id.clone());
            }
        }

        if self.targets.contains(&ctx_task_id) && !self.completed.contains(&ctx_task_id) {
            return Ok((ctx_task_id, vec![]));
        }

        self.clear_task_state(&ctx_task_id);

        let new_resolved
            = project.resolve_task(&task_id)?;

        let mut resolved_ctx_task_ids: Vec<ContextualTaskId>
            = Vec::new();

        for (tid, prereqs) in new_resolved.tasks {
            let ctx_tid
                = ContextualTaskId::new(tid.clone(), ctx_id.clone());

            self.clear_task_state(&ctx_tid);
            resolved_ctx_task_ids.push(ctx_tid);
            self.resolved.tasks.entry(tid).or_insert(prereqs);
        }

        for (ident, tf) in new_resolved.task_files {
            self.resolved.task_files.entry(ident).or_insert(tf);
        }

        self.targets.insert(ctx_task_id.clone());

        self.prepare_new_tasks(project, &ctx_id)?;

        if !args.is_empty() {
            if let Some(task) = self.prepared.get_mut(&ctx_task_id) {
                task.args = args;
            }
        }

        Ok((ctx_task_id, resolved_ctx_task_ids))
    }

    pub fn prepare_new_tasks(&mut self, project: &Project, context_id: &str) -> Result<usize, Error> {
        let colors: Vec<&DataType>
            = prefix_colors().take(5).collect();

        let mut color_index
            = self.prepared.len();

        let mut new_count
            = 0;

        let task_ids: Vec<TaskId>
            = self.resolved.tasks.keys().cloned().collect();

        for task_id in task_ids {
            let ctx_task_id
                = ContextualTaskId::new(task_id.clone(), context_id.to_string());

            if self.prepared.contains_key(&ctx_task_id) {
                continue;
            }

            let Some(task_file) = self.resolved.task_files.get(&task_id.workspace) else {
                continue;
            };

            let Some(task) = task_file.tasks.get(task_id.task_name.as_str()) else {
                continue;
            };

            if task.script.is_empty() {
                continue;
            }

            let Ok(workspace) = project.workspace_by_ident(&task_id.workspace) else {
                continue;
            };

            let script
                = task.script.join("\n");

            let mut env
                = BTreeMap::new();

            env.insert(
                "npm_lifecycle_event".to_string(),
                task_id.task_name.as_str().to_string(),
            );

            let color
                = colors[color_index % colors.len()];

            color_index += 1;

            let prefix
                = color.colorize(&format!(
                    "[{}:{}]: ",
                    task_id.workspace.to_file_string(),
                    task_id.task_name.as_str()
                ));

            let is_long_lived
                = task.attributes.iter().any(|attr| attr.name == "long-lived");

            self.prepared.insert(
                ctx_task_id,
                PreparedTask {
                    script,
                    cwd: workspace.path.clone(),
                    env,
                    prefix,
                    args: vec![],
                    is_long_lived,
                },
            );

            new_count += 1;
        }

        Ok(new_count)
    }

    pub fn is_task_fully_completed(&self, task_id: &ContextualTaskId) -> bool {
        if !self.script_finished.contains(task_id) {
            return false;
        }

        if let Some(task_subtasks) = self.subtasks.get(task_id) {
            task_subtasks.iter().all(|s| self.completed.contains(s))
        } else {
            true
        }
    }

    pub fn try_complete_task(&mut self, task_id: &ContextualTaskId) -> bool {
        if self.is_task_fully_completed(task_id) {
            self.completed.insert(task_id.clone());
            true
        } else {
            false
        }
    }

    pub fn all_targets_completed(&self) -> bool {
        self.targets.iter().all(|t| self.completed.contains(t))
    }

    fn clear_task_state(&mut self, task_id: &ContextualTaskId) {
        self.completed.remove(task_id);
        self.script_finished.remove(task_id);
        self.failed.remove(task_id);
        self.warm_up_complete.remove(task_id);
        self.targets.remove(task_id);
        self.subtasks.remove(task_id);
    }

    fn parse_contextual_task_id(&self, project: &Project, task_id_str: &str) -> Option<ContextualTaskId> {
        let (task_part, context_id)
            = task_id_str.rsplit_once('@')?;

        let (workspace_str, task_name_str)
            = task_part.split_once(':')?;

        let task_name
            = TaskName::new(task_name_str).ok()?;

        let ident
            = Ident::new(workspace_str);

        let workspace
            = project.workspace_by_ident(&ident).ok()?;

        Some(ContextualTaskId::new(
            TaskId {
                workspace: workspace.name.clone(),
                task_name,
            },
            context_id.to_string(),
        ))
    }

    fn parse_context_id(&self, task_id_str: &str) -> Option<String> {
        let (_, context_id)
            = task_id_str.rsplit_once('@')?;

        Some(context_id.to_string())
    }
}
