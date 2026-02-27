use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::{atomic::{AtomicUsize, Ordering}, Mutex, RwLock};

use zpm_primitives::Ident;
use zpm_tasks::{TaskId, TaskName};
use zpm_utils::{DataType, Path, ToFileString};

use crate::error::Error;
use crate::project::Project;

fn interpolate_gradient(keyframes: &[(u8, u8, u8)], steps_between: usize) -> Vec<(u8, u8, u8)> {
    let mut colors = Vec::with_capacity(keyframes.len() * steps_between);

    for i in 0..keyframes.len() {
        let (r1, g1, b1) = keyframes[i];
        let (r2, g2, b2) = keyframes[(i + 1) % keyframes.len()];

        for step in 0..steps_between {
            let t = step as f32 / steps_between as f32;

            let r = (r1 as f32 + (r2 as f32 - r1 as f32) * t) as u8;
            let g = (g1 as f32 + (g2 as f32 - g1 as f32) * t) as u8;
            let b = (b1 as f32 + (b2 as f32 - b1 as f32) * t) as u8;

            colors.push((r, g, b));
        }
    }

    colors
}

fn generate_gradient_frames(text: &str) -> Vec<String> {
    let keyframes: [(u8, u8, u8); 4] = [
        (100, 149, 237),
        (65, 105, 225),
        (30, 144, 255),
        (0, 191, 255),
    ];

    let gradient_colors = interpolate_gradient(&keyframes, 8);

    let chars: Vec<char> = text.chars().collect();

    (0..gradient_colors.len())
        .map(|frame| {
            let mut result = String::with_capacity(text.len() * 20);

            for (i, ch) in chars.iter().enumerate() {
                let color_idx = (i * 2 + gradient_colors.len() - frame) % gradient_colors.len();

                let (r, g, b) = gradient_colors[color_idx];

                result.push_str(&format!("\x1b[38;2;{};{};{}m{}", r, g, b, ch));
            }

            result.push_str("\x1b[0m");
            result
        })
        .collect()
}

pub struct ProgressState {
    pub total: AtomicUsize,
    pub completed: AtomicUsize,
    pub running_tasks: Mutex<BTreeSet<String>>,
    gradient_frames: Vec<String>,
}

impl ProgressState {
    pub fn new(total: usize) -> Self {
        let gradient_frames = generate_gradient_frames("Running dependencies");

        Self {
            total: AtomicUsize::new(total),
            completed: AtomicUsize::new(0),
            running_tasks: Mutex::new(BTreeSet::new()),
            gradient_frames,
        }
    }

    pub fn add_to_total(&self, count: usize) {
        self.total.fetch_add(count, Ordering::Relaxed);
    }

    pub fn add_task(&self, task_name: &str) {
        self.running_tasks.lock().unwrap().insert(task_name.to_string());
    }

    pub fn remove_task(&self, task_name: &str) {
        self.running_tasks.lock().unwrap().remove(task_name);
        self.completed.fetch_add(1, Ordering::Relaxed);
    }

    pub fn format_progress(&self, frame_idx: usize) -> String {
        let total = self.total.load(Ordering::Relaxed);
        let completed = self.completed.load(Ordering::Relaxed);
        let running = self.running_tasks.lock().unwrap().len();
        let scheduled = total.saturating_sub(running).saturating_sub(completed);

        let label = &self.gradient_frames[frame_idx % self.gradient_frames.len()];

        format!(
            "{} {}",
            label,
            DataType::Custom(128, 128, 128).colorize(&format!(
                "· running {} · scheduled {} · completed {}",
                running,
                scheduled,
                completed
            ))
        )
    }
}

pub fn prefix_colors() -> impl Iterator<Item = &'static DataType> {
    static COLORS: [DataType; 5] = [
        DataType::Custom(46, 134, 171),
        DataType::Custom(162, 59, 114),
        DataType::Custom(241, 143, 1),
        DataType::Custom(199, 62, 29),
        DataType::Custom(204, 226, 163),
    ];

    COLORS.iter().cycle()
}

#[derive(Clone)]
pub struct PreparedTask {
    pub script: String,
    pub cwd: Path,
    pub env: BTreeMap<String, String>,
    pub prefix: String,
}

pub struct DynamicExecutionState {
    pub resolved: RwLock<zpm_tasks::ResolvedTasks>,
    pub target_tasks: RwLock<HashSet<TaskId>>,
    pub original_targets: RwLock<HashSet<TaskId>>,
    pub completed: RwLock<HashSet<TaskId>>,
    pub script_finished: RwLock<HashSet<TaskId>>,
    pub subtasks: RwLock<HashMap<TaskId, HashSet<TaskId>>>,
    pub prepared_tasks: RwLock<BTreeMap<TaskId, PreparedTask>>,
    pub color_index: RwLock<usize>,
}

impl DynamicExecutionState {
    pub fn empty() -> Self {
        Self {
            resolved: RwLock::new(zpm_tasks::ResolvedTasks {
                tasks: BTreeMap::new(),
                task_files: BTreeMap::new(),
            }),
            target_tasks: RwLock::new(HashSet::new()),
            original_targets: RwLock::new(HashSet::new()),
            completed: RwLock::new(HashSet::new()),
            script_finished: RwLock::new(HashSet::new()),
            subtasks: RwLock::new(HashMap::new()),
            prepared_tasks: RwLock::new(BTreeMap::new()),
            color_index: RwLock::new(0),
        }
    }

    pub fn new(resolved: zpm_tasks::ResolvedTasks, root_task: TaskId) -> Self {
        let mut target_tasks = HashSet::new();
        target_tasks.insert(root_task.clone());

        let mut original_targets = HashSet::new();
        original_targets.insert(root_task);

        Self {
            resolved: RwLock::new(resolved),
            target_tasks: RwLock::new(target_tasks),
            original_targets: RwLock::new(original_targets),
            completed: RwLock::new(HashSet::new()),
            script_finished: RwLock::new(HashSet::new()),
            subtasks: RwLock::new(HashMap::new()),
            prepared_tasks: RwLock::new(BTreeMap::new()),
            color_index: RwLock::new(0),
        }
    }

    pub fn all_targets_completed(&self) -> bool {
        let targets = self.target_tasks.read().unwrap();
        let completed = self.completed.read().unwrap();

        targets.iter().all(|t| completed.contains(t))
    }

    pub fn is_task_fully_completed(&self, task_id: &TaskId) -> bool {
        let script_finished = self.script_finished.read().unwrap();

        if !script_finished.contains(task_id) {
            return false;
        }

        let subtasks = self.subtasks.read().unwrap();
        let completed = self.completed.read().unwrap();

        if let Some(task_subtasks) = subtasks.get(task_id) {
            task_subtasks.iter().all(|s| completed.contains(s))
        } else {
            true
        }
    }

    pub fn try_complete_task(&self, task_id: &TaskId) -> bool {
        if self.is_task_fully_completed(task_id) {
            let mut completed = self.completed.write().unwrap();
            completed.insert(task_id.clone());
            true
        } else {
            false
        }
    }

    pub fn add_pushed_task(&self, project: &Project, task_name: &str, parent_task_id: Option<&str>) -> Result<(TaskId, usize), Error> {
        let task_name = TaskName::new(task_name)
            .map_err(|_| Error::TaskNameParseError(task_name.to_string()))?;

        let workspace = project.active_workspace()?;

        let task_id = TaskId {
            workspace: workspace.name.clone(),
            task_name,
        };

        if let Some(parent_str) = parent_task_id {
            if let Some(parent_id) = self.parse_task_id(project, parent_str) {
                let mut subtasks = self.subtasks.write().unwrap();

                subtasks
                    .entry(parent_id)
                    .or_default()
                    .insert(task_id.clone());
            }
        }

        {
            let completed = self.completed.read().unwrap();
            let targets = self.target_tasks.read().unwrap();

            if completed.contains(&task_id) || targets.contains(&task_id) {
                return Ok((task_id, 0));
            }
        }

        let new_resolved = project.resolve_task(&task_id)?;

        {
            let mut resolved = self.resolved.write().unwrap();

            for (tid, prereqs) in new_resolved.tasks {
                resolved.tasks.entry(tid).or_insert(prereqs);
            }

            for (ident, tf) in new_resolved.task_files {
                resolved.task_files.entry(ident).or_insert(tf);
            }
        }

        {
            let mut targets = self.target_tasks.write().unwrap();
            targets.insert(task_id.clone());
        }

        let new_task_count = self.prepare_new_tasks(project)?;

        Ok((task_id, new_task_count))
    }

    pub fn prepare_new_tasks(&self, project: &Project) -> Result<usize, Error> {
        let resolved = self.resolved.read().unwrap();
        let mut prepared = self.prepared_tasks.write().unwrap();
        let mut color_index = self.color_index.write().unwrap();

        let colors: Vec<&DataType> = prefix_colors().take(5).collect();

        let mut new_count = 0;

        for task_id in resolved.tasks.keys() {
            if prepared.contains_key(task_id) {
                continue;
            }

            let Some(task_file) = resolved.task_files.get(&task_id.workspace) else {
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

            let color = colors[*color_index % colors.len()];

            *color_index += 1;

            let prefix = color.colorize(&format!(
                "[{}:{}]: ",
                task_id.workspace.to_file_string(),
                task_id.task_name.as_str()
            ));

            prepared.insert(
                task_id.clone(),
                PreparedTask {
                    script,
                    cwd: workspace.path.clone(),
                    env,
                    prefix,
                },
            );

            new_count += 1;
        }

        Ok(new_count)
    }

    pub fn parse_task_id(&self, project: &Project, task_id_str: &str) -> Option<TaskId> {
        let (workspace_str, task_name_str) = task_id_str.split_once(':')?;

        let task_name = TaskName::new(task_name_str).ok()?;

        let ident = Ident::new(workspace_str);

        let workspace = project.workspace_by_ident(&ident).ok()?;

        Some(TaskId {
            workspace: workspace.name.clone(),
            task_name,
        })
    }
}
