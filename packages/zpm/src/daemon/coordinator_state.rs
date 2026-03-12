// ============================================================================
// Unified Coordinator State
//
// This module consolidates all mutable daemon state into a single struct
// that is owned exclusively by the coordinator loop. No Arc<RwLock> wrappers -
// the coordinator is the single owner, making race conditions structurally
// impossible.
// ============================================================================

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::time::{Duration, Instant, SystemTime};

use tokio::sync::mpsc;
use zpm_primitives::Ident;
use zpm_tasks::{ResolvedTasks, TaskId, TaskName};
use zpm_utils::{DataType, ToFileString};

use super::ipc::{BufferedOutputLine, DaemonNotification, SubscriptionScope};
use super::presentation::prefix_colors;
pub use super::scheduler::{ContextualTaskId, PreparedTask};
use crate::error::Error;
use crate::project::Project;

// ============================================================================
// Long-Lived Task State
// ============================================================================

#[derive(Debug, Clone)]
pub struct LongLivedEntry {
    pub task_id: TaskId,
    pub contextual_task_id: String,
    pub warm_up_complete: bool,
    pub process_id: Option<u32>,
    pub started_at: SystemTime,
}

struct LongLivedRegistration {
    entry: LongLivedEntry,
    claimed_at: Option<Instant>,
}

// ============================================================================
// Subscription State
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SubscriptionId(pub u64);

#[derive(Debug, Clone)]
pub struct SubscriptionFilter {
    pub output_scope: SubscriptionScope,
    pub status_scope: SubscriptionScope,
    pub target_task_ids: HashSet<String>,
    pub all_task_ids: HashSet<String>,
    pub context_id: Option<String>,
}

impl SubscriptionFilter {
    pub fn new(output_scope: SubscriptionScope, status_scope: SubscriptionScope, context_id: Option<String>) -> Self {
        Self {
            output_scope,
            status_scope,
            target_task_ids: HashSet::new(),
            all_task_ids: HashSet::new(),
            context_id,
        }
    }

    pub fn matches(&self, notification: &DaemonNotification) -> bool {
        let (task_id, scope) = match notification {
            DaemonNotification::TaskOutputLine { task_id, .. } => (task_id, self.output_scope),
            DaemonNotification::TaskStarted { task_id } => (task_id, self.status_scope),
            DaemonNotification::TaskCompleted { task_id, .. } => (task_id, self.status_scope),
            DaemonNotification::TaskFailed { task_id, .. } => (task_id, self.status_scope),
            DaemonNotification::TaskWarmUpComplete { task_id } => (task_id, self.status_scope),
        };

        let is_explicit_target = self.target_task_ids.contains(task_id);

        if let Some(ref ctx) = self.context_id {
            if !is_explicit_target && !task_id.ends_with(&format!("@{}", ctx)) {
                return false;
            }
        }

        match scope {
            SubscriptionScope::None => false,
            SubscriptionScope::TargetOnly => is_explicit_target,
            SubscriptionScope::FullTree => {
                if is_explicit_target {
                    return true;
                }
                match &self.context_id {
                    Some(ctx) => task_id.ends_with(&format!("@{}", ctx)),
                    None => true,
                }
            }
        }
    }

    pub fn add_target_task(&mut self, task_id: String) {
        self.target_task_ids.insert(task_id.clone());
        self.all_task_ids.insert(task_id);
    }

    pub fn add_dependency_task(&mut self, task_id: String) {
        self.all_task_ids.insert(task_id);
    }
}

struct Subscription {
    filter: SubscriptionFilter,
    sender: mpsc::UnboundedSender<DaemonNotification>,
}

// ============================================================================
// Spawning Task State
// ============================================================================

#[derive(Debug)]
struct SpawningEntry {
    spawned_at: Instant,
    pending_cancel: bool,
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

// ============================================================================
// Unified Coordinator State
// ============================================================================

/// All mutable daemon state in one place.
/// Only modified by the coordinator event loop - no locks needed.
pub struct CoordinatorState {
    // ========== SCHEDULER STATE ==========
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
    /// Long-lived tasks that have completed their warm-up period
    pub warm_up_complete: HashSet<ContextualTaskId>,
    /// Parent-child subtask relationships
    pub subtasks: HashMap<ContextualTaskId, HashSet<ContextualTaskId>>,
    /// Prepared task execution info
    pub prepared: BTreeMap<ContextualTaskId, PreparedTask>,

    // ========== PROCESS REGISTRY STATE ==========
    /// All registered PIDs
    pids: HashSet<u32>,
    /// Mapping from task_id string to PID
    task_to_pid: HashMap<String, u32>,

    // ========== SPAWNING TASKS STATE ==========
    /// Tasks currently spawning (between spawn() and PID registration)
    spawning: HashMap<String, SpawningEntry>,

    // ========== LONG-LIVED REGISTRY STATE ==========
    /// Long-lived task entries
    long_lived_entries: HashMap<TaskId, LongLivedRegistration>,

    // ========== WARM-UP TRACKING ==========
    /// Pending warm-up deadlines (replaces spawned timer tasks)
    warm_up_deadlines: Vec<WarmUpDeadline>,

    // ========== SUBSCRIPTION STATE ==========
    /// Active subscriptions
    subscriptions: HashMap<SubscriptionId, Subscription>,
    /// Next subscription ID
    next_subscription_id: u64,

    // ========== OUTPUT BUFFER STATE ==========
    /// Output lines per task
    output_buffer: HashMap<String, Vec<BufferedOutputLine>>,
    /// Closed tasks in order (for cleanup)
    closed_tasks: VecDeque<String>,
    /// Max lines per task
    output_buffer_max_lines: usize,
    /// Max closed tasks to keep
    max_closed_tasks: usize,
}

impl CoordinatorState {
    pub fn new(output_buffer_max_lines: usize, max_closed_tasks: usize) -> Self {
        Self {
            // Scheduler state
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

            // Process registry
            pids: HashSet::new(),
            task_to_pid: HashMap::new(),

            // Spawning tasks
            spawning: HashMap::new(),

            // Long-lived registry
            long_lived_entries: HashMap::new(),

            // Warm-up tracking
            warm_up_deadlines: Vec::new(),

            // Subscriptions
            subscriptions: HashMap::new(),
            next_subscription_id: 1,

            // Output buffer
            output_buffer: HashMap::new(),
            closed_tasks: VecDeque::new(),
            output_buffer_max_lines,
            max_closed_tasks,
        }
    }

    // ========================================================================
    // Scheduler Operations
    // ========================================================================

    pub fn add_task(
        &mut self,
        project: &Project,
        task_name: &str,
        parent_task_id: Option<&str>,
        args: Vec<String>,
        workspace_override: Option<&str>,
        context_id: Option<&str>,
    ) -> Result<(ContextualTaskId, Vec<ContextualTaskId>), Error> {
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
            self.parse_context_id(parent_str)
                .ok_or_else(|| Error::MissingContextId)?
        } else {
            return Err(Error::MissingContextId);
        };

        let ctx_task_id = ContextualTaskId::new(task_id.clone(), ctx_id.clone());

        if let Some(parent_str) = parent_task_id {
            if let Some(parent_ctx_id) = self.parse_contextual_task_id(project, parent_str) {
                self.subtasks
                    .entry(parent_ctx_id)
                    .or_default()
                    .insert(ctx_task_id.clone());
            }
        }

        // If task is already in targets for this context, don't re-add
        if self.targets.contains(&ctx_task_id) {
            return Ok((ctx_task_id, vec![]));
        }

        self.clear_task_state(&ctx_task_id);

        let new_resolved = project.resolve_task(&task_id)?;

        let mut resolved_ctx_task_ids: Vec<ContextualTaskId> = Vec::new();

        for (tid, prereqs) in new_resolved.tasks {
            let ctx_tid = ContextualTaskId::new(tid.clone(), ctx_id.clone());
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

    fn prepare_new_tasks(&mut self, project: &Project, context_id: &str) -> Result<usize, Error> {
        let colors: Vec<&DataType> = prefix_colors().take(5).collect();
        let mut color_index = self.prepared.len();
        let mut new_count = 0;

        let task_ids: Vec<TaskId> = self.resolved.tasks.keys().cloned().collect();

        for task_id in task_ids {
            let ctx_task_id = ContextualTaskId::new(task_id.clone(), context_id.to_string());

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

            let is_long_lived = task.attributes.iter().any(|attr| attr.name == "long-lived");

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

    fn clear_task_state(&mut self, task_id: &ContextualTaskId) {
        self.completed.remove(task_id);
        self.script_finished.remove(task_id);
        self.failed.remove(task_id);
        self.warm_up_complete.remove(task_id);
        self.targets.remove(task_id);
        self.subtasks.remove(task_id);
    }

    pub fn is_task_fully_completed(&self, task_id: &ContextualTaskId) -> bool {
        if !self.script_finished.contains(task_id) {
            return false;
        }

        if let Some(task_subtasks) = self.subtasks.get(task_id) {
            task_subtasks.iter().all(|s| self.completed.contains(s) && !self.failed.contains(s))
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

    pub fn mark_script_finished(&mut self, task_id: &ContextualTaskId) {
        self.script_finished.insert(task_id.clone());
    }

    pub fn mark_completed(&mut self, task_id: &ContextualTaskId) {
        self.completed.insert(task_id.clone());
    }

    pub fn mark_failed(&mut self, task_id: &ContextualTaskId) {
        self.failed.insert(task_id.clone());
        self.completed.insert(task_id.clone());
    }

    pub fn is_long_lived(&self, task_id: &ContextualTaskId) -> bool {
        self.prepared
            .get(task_id)
            .map(|p| p.is_long_lived)
            .unwrap_or(false)
    }

    pub fn has_failed_subtask(&self, task_id: &ContextualTaskId) -> bool {
        if let Some(subtasks) = self.subtasks.get(task_id) {
            subtasks.iter().any(|s| self.failed.contains(s))
        } else {
            false
        }
    }

    pub fn find_parents(&self, task_id: &ContextualTaskId) -> Vec<ContextualTaskId> {
        self.subtasks
            .iter()
            .filter(|(_, children)| children.contains(task_id))
            .map(|(parent, _)| parent.clone())
            .collect()
    }

    /// Atomically check if a task should be spawned.
    /// No race possible - we own the state.
    pub fn should_spawn_task(&self, task_id: &ContextualTaskId) -> bool {
        !self.completed.contains(task_id) && !self.failed.contains(task_id)
    }

    /// Cancel all tasks in a context. Returns cancelled task IDs.
    pub fn cancel_context(&mut self, context_id: &str) -> Vec<String> {
        let tasks_to_cancel: Vec<ContextualTaskId> = self
            .prepared
            .keys()
            .filter(|ctx_task_id| {
                ctx_task_id.context_id == context_id && !self.completed.contains(ctx_task_id)
            })
            .cloned()
            .collect();

        let mut cancelled_ids = Vec::new();

        for task_id in tasks_to_cancel {
            self.failed.insert(task_id.clone());
            self.completed.insert(task_id.clone());
            cancelled_ids.push(format_contextual_task_id(&task_id));
        }

        cancelled_ids
    }

    fn parse_contextual_task_id(&self, project: &Project, task_id_str: &str) -> Option<ContextualTaskId> {
        let (task_part, context_id) = task_id_str.rsplit_once('@')?;
        let (workspace_str, task_name_str) = task_part.split_once(':')?;

        let task_name = TaskName::new(task_name_str).ok()?;
        let ident = Ident::new(workspace_str);
        let workspace = project.workspace_by_ident(&ident).ok()?;

        Some(ContextualTaskId::new(
            TaskId {
                workspace: workspace.name.clone(),
                task_name,
            },
            context_id.to_string(),
        ))
    }

    fn parse_context_id(&self, task_id_str: &str) -> Option<String> {
        let (_, context_id) = task_id_str.rsplit_once('@')?;
        Some(context_id.to_string())
    }

    pub fn parse_contextual_task_id_simple(&self, task_id_str: &str) -> Option<ContextualTaskId> {
        let (task_part, context_id) = task_id_str.rsplit_once('@')?;
        let (workspace_str, task_name_str) = task_part.split_once(':')?;

        let task_name = TaskName::new(task_name_str).ok()?;
        let workspace = Ident::new(workspace_str);

        Some(ContextualTaskId::new(
            TaskId { workspace, task_name },
            context_id.to_string(),
        ))
    }

    // ========================================================================
    // Process Registry Operations
    // ========================================================================

    pub fn register_pid(&mut self, pid: u32, task_id: String) {
        self.pids.insert(pid);
        self.task_to_pid.insert(task_id, pid);
    }

    pub fn unregister_pid(&mut self, pid: u32, task_id: &str) {
        self.pids.remove(&pid);
        self.task_to_pid.remove(task_id);
    }

    pub fn get_all_pids(&self) -> Vec<u32> {
        self.pids.iter().cloned().collect()
    }

    pub fn take_pid_for_task(&mut self, task_id: &str) -> Option<u32> {
        let pid = self.task_to_pid.remove(task_id)?;
        self.pids.remove(&pid);
        Some(pid)
    }

    pub fn take_pids_for_context(&mut self, context_id: &str) -> Vec<u32> {
        let suffix = format!("@{}", context_id);
        let task_ids_to_remove: Vec<String> = self
            .task_to_pid
            .keys()
            .filter(|task_id| task_id.ends_with(&suffix))
            .cloned()
            .collect();

        let mut pids = Vec::with_capacity(task_ids_to_remove.len());
        for task_id in task_ids_to_remove {
            if let Some(pid) = self.task_to_pid.remove(&task_id) {
                self.pids.remove(&pid);
                pids.push(pid);
            }
        }

        pids
    }

    // ========================================================================
    // Spawning Tasks Operations
    // ========================================================================

    pub fn mark_spawning(&mut self, task_id: String) {
        self.spawning.insert(task_id, SpawningEntry {
            spawned_at: Instant::now(),
            pending_cancel: false,
        });
    }

    pub fn mark_spawning_pending_cancel(&mut self, task_id: &str) -> bool {
        if let Some(entry) = self.spawning.get_mut(task_id) {
            entry.pending_cancel = true;
            true
        } else {
            false
        }
    }

    pub fn take_spawning(&mut self, task_id: &str) -> Option<bool> {
        self.spawning.remove(task_id).map(|e| e.pending_cancel)
    }

    pub fn get_spawning_for_context(&self, context_id: &str) -> Vec<String> {
        let suffix = format!("@{}", context_id);
        self.spawning
            .keys()
            .filter(|id| id.ends_with(&suffix))
            .cloned()
            .collect()
    }

    // ========================================================================
    // Long-Lived Registry Operations
    // ========================================================================

    pub fn get_long_lived(&self, task_id: &TaskId) -> Option<LongLivedEntry> {
        self.long_lived_entries.get(task_id).map(|r| r.entry.clone())
    }

    pub fn register_long_lived(&mut self, task_id: TaskId, contextual_task_id: String) {
        self.long_lived_entries.insert(
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

    pub fn remove_long_lived(&mut self, task_id: &TaskId) -> Option<LongLivedEntry> {
        self.long_lived_entries.remove(task_id).map(|r| r.entry)
    }

    pub fn mark_long_lived_warm_up_complete(&mut self, task_id: &TaskId) -> bool {
        if let Some(reg) = self.long_lived_entries.get_mut(task_id) {
            reg.entry.warm_up_complete = true;
            true
        } else {
            false
        }
    }

    pub fn list_long_lived(&self) -> Vec<LongLivedEntry> {
        self.long_lived_entries
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

    /// Process warm-up deadlines and return tasks that completed warm-up.
    pub fn process_warm_up_deadlines(&mut self) -> Vec<(ContextualTaskId, TaskId)> {
        let now = Instant::now();
        let mut completed = Vec::new();

        self.warm_up_deadlines.retain(|deadline| {
            if now >= deadline.deadline {
                // Check if task hasn't failed/completed during warm-up
                if !self.failed.contains(&deadline.contextual_task_id)
                    && !self.completed.contains(&deadline.contextual_task_id)
                {
                    completed.push((deadline.contextual_task_id.clone(), deadline.base_task_id.clone()));
                }
                false // Remove from list
            } else {
                true // Keep in list
            }
        });

        completed
    }

    // ========================================================================
    // Subscription Operations
    // ========================================================================

    pub fn create_subscription(
        &mut self,
        output_scope: SubscriptionScope,
        status_scope: SubscriptionScope,
        context_id: Option<String>,
    ) -> (SubscriptionId, mpsc::UnboundedReceiver<DaemonNotification>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let filter = SubscriptionFilter::new(output_scope, status_scope, context_id);

        let id = SubscriptionId(self.next_subscription_id);
        self.next_subscription_id += 1;

        self.subscriptions.insert(id, Subscription { filter, sender: tx });

        (id, rx)
    }

    pub fn add_tasks_to_subscription(
        &mut self,
        subscription_id: SubscriptionId,
        target_task_ids: Vec<String>,
        dependency_task_ids: Vec<String>,
    ) {
        if let Some(sub) = self.subscriptions.get_mut(&subscription_id) {
            for task_id in target_task_ids {
                sub.filter.add_target_task(task_id);
            }
            for task_id in dependency_task_ids {
                sub.filter.add_dependency_task(task_id);
            }
        }
    }

    pub fn remove_subscription(&mut self, subscription_id: SubscriptionId) {
        self.subscriptions.remove(&subscription_id);
    }

    pub fn broadcast(&self, notification: DaemonNotification) {
        for sub in self.subscriptions.values() {
            if sub.filter.matches(&notification) {
                let _ = sub.sender.send(notification.clone());
            }
        }
    }

    // ========================================================================
    // Output Buffer Operations
    // ========================================================================

    pub fn buffer_output(&mut self, task_id: String, line: BufferedOutputLine) {
        let lines = self.output_buffer.entry(task_id).or_default();
        lines.push(line);

        if lines.len() > self.output_buffer_max_lines {
            let excess = lines.len() - self.output_buffer_max_lines;
            lines.drain(0..excess);
        }
    }

    pub fn get_task_output(&self, task_id: &str) -> Vec<BufferedOutputLine> {
        self.output_buffer.get(task_id).cloned().unwrap_or_default()
    }

    pub fn mark_task_closed(&mut self, task_id: String) {
        self.closed_tasks.push_back(task_id);

        // Clean up oldest closed task buffers if we exceed the limit
        while self.closed_tasks.len() > self.max_closed_tasks {
            if let Some(oldest_task_id) = self.closed_tasks.pop_front() {
                self.output_buffer.remove(&oldest_task_id);
            }
        }
    }
}

// ============================================================================
// Formatting Helpers
// ============================================================================

pub fn format_contextual_task_id(ctx_task_id: &ContextualTaskId) -> String {
    format!(
        "{}:{}@{}",
        ctx_task_id.task_id.workspace.to_file_string(),
        ctx_task_id.task_id.task_name.as_str(),
        ctx_task_id.context_id
    )
}

pub fn parse_base_task_id(contextual_task_id: &str) -> Option<TaskId> {
    let (task_part, _context_id) = contextual_task_id.rsplit_once('@')?;
    let (workspace_str, task_name_str) = task_part.split_once(':')?;

    let task_name = TaskName::new(task_name_str).ok()?;
    let workspace = Ident::new(workspace_str);

    Some(TaskId { workspace, task_name })
}
