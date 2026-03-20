use std::{collections::{BTreeMap, BTreeSet, HashMap, HashSet}, io::Write, os::unix::process::ExitStatusExt, process::ExitStatus, sync::{atomic::{AtomicBool, AtomicUsize, Ordering}, Arc, Mutex, RwLock}, time::Instant};

use clipanion::cli;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::mpsc;
use zpm_tasks::{parse, TaskId, TaskName};
use zpm_utils::{is_terminal, shell_escape, start_progress, DataType, Path, ProgressHandle, ToFileString, ToHumanString, Unit};

use crate::{error::Error, ipc::{IPC_SOCKET_ENV, IPC_CURRENT_TASK_ENV, PushRequest, PushResponse, TaskIpcServer}, project::Project, script::ScriptEnvironment};

#[derive(Clone)]
struct SpawnedTaskOptions {
    verbose_level: u8,
    interlaced: bool,
    enable_timers: bool,
    silent_dependencies: bool,
}

struct ProgressState {
    total: AtomicUsize,
    completed: AtomicUsize,
    running_tasks: Mutex<BTreeSet<String>>,
    gradient_frames: Vec<String>,
}

fn interpolate_gradient(keyframes: &[(u8, u8, u8)], steps_between: usize) -> Vec<(u8, u8, u8)> {
    let mut colors
        = Vec::with_capacity(keyframes.len() * steps_between);

    for i in 0..keyframes.len() {
        let (r1, g1, b1) = keyframes[i];
        let (r2, g2, b2) = keyframes[(i + 1) % keyframes.len()];

        for step in 0..steps_between {
            let t
                = step as f32 / steps_between as f32;

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

    let gradient_colors
        = interpolate_gradient(&keyframes, 8);

    let chars: Vec<char>
        = text.chars().collect();

    (0..gradient_colors.len())
        .map(|frame| {
            let mut result
                = String::with_capacity(text.len() * 20);

            for (i, ch) in chars.iter().enumerate() {
                let color_idx
                    = (i * 2 + gradient_colors.len() - frame) % gradient_colors.len();

                let (r, g, b)
                    = gradient_colors[color_idx];

                result.push_str(&format!("\x1b[38;2;{};{};{}m{}", r, g, b, ch));
            }

            result.push_str("\x1b[0m");
            result
        })
        .collect()
}

impl ProgressState {
    fn new(total: usize) -> Self {
        let gradient_frames
            = generate_gradient_frames("Running dependencies");

        Self {
            total: AtomicUsize::new(total),
            completed: AtomicUsize::new(0),
            running_tasks: Mutex::new(BTreeSet::new()),
            gradient_frames,
        }
    }

    fn add_to_total(&self, count: usize) {
        self.total.fetch_add(count, Ordering::Relaxed);
    }

    fn add_task(&self, task_name: &str) {
        self.running_tasks.lock().unwrap().insert(task_name.to_string());
    }

    fn remove_task(&self, task_name: &str) {
        self.running_tasks.lock().unwrap().remove(task_name);
        self.completed.fetch_add(1, Ordering::Relaxed);
    }

    fn format_progress(&self, frame_idx: usize) -> String {
        let total
            = self.total.load(Ordering::Relaxed);

        let completed
            = self.completed.load(Ordering::Relaxed);

        let running
            = self.running_tasks.lock().unwrap().len();

        let scheduled
            = total.saturating_sub(running).saturating_sub(completed);

        let label
            = &self.gradient_frames[frame_idx % self.gradient_frames.len()];

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

fn prefix_colors() -> impl Iterator<Item = &'static DataType> {
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
struct PreparedTask {
    script: String,
    cwd: Path,
    env: BTreeMap<String, String>,
    prefix: String,
}

#[cli::command(proxy)]
#[cli::path("tasks", "run")]
#[cli::category("Scripting commands")]
pub struct TaskRun {
    #[cli::option("-i,--interlaced", default = true)]
    interlaced: bool,

    #[cli::option("-v,--verbose", default = if zpm_utils::is_terminal() {2} else {0}, counter)]
    verbose_level: u8,

    #[cli::option("--silent-dependencies", default = false)]
    silent_dependencies: bool,

    name: String,
    args: Vec<String>,
}

impl TaskRun {
    pub async fn execute(&self) -> Result<ExitStatus, Error> {
        let mut project
            = Project::new(None).await?;

        project
            .lazy_install().await?;

        let options
            = SpawnedTaskOptions {
                verbose_level: self.verbose_level,
                interlaced: self.interlaced,
                enable_timers: project.config.settings.enable_timers.value,
                silent_dependencies: self.silent_dependencies,
            };

        run_task_impl(&project, &self.name, &self.args, &options).await
    }
}

pub async fn run_task(
    project: &Project,
    name: &str,
    args: &[String],
    verbose_level: u8,
    silent_dependencies: bool,
    interlaced: bool,
    enable_timers: bool,
) -> Result<ExitStatus, Error> {
    let options
        = SpawnedTaskOptions {
            verbose_level,
            interlaced,
            enable_timers,
            silent_dependencies,
        };

    run_task_impl(project, name, args, &options).await
}

async fn run_task_impl(
    project: &Project,
    name: &str,
    args: &[String],
    options: &SpawnedTaskOptions,
) -> Result<ExitStatus, Error> {
    let task_name
        = TaskName::new(name)
            .map_err(|_| Error::TaskNameParseError(name.to_string()))?;

    let workspace
        = project.active_workspace()?;

    let task_file_path
        = workspace.taskfile_path();

    if !task_file_path.fs_exists().await {
        return Err(Error::TaskFileNotFound(workspace.path.clone()));
    }

    let task_file_content
        = task_file_path.fs_read_text().await?;

    let task_file
        = parse(&task_file_content).map_err(Error::TaskParseError)?;

    if !task_file.tasks.contains_key(task_name.as_str()) {
        return Err(Error::TaskNotFound {
            workspace: workspace.name.clone(),
            task_name: name.to_string(),
        });
    }

    let root_task
        = TaskId {
            workspace: workspace.name.clone(),
            task_name,
        };

    let resolved
        = project.resolve_task(&root_task)?;

    let ipc_server
        = TaskIpcServer::new().await?;

    let socket_name
        = ipc_server.socket_name().to_string();

    let (push_tx, push_rx)
        = mpsc::channel::<PushRequest>(32);

    let ipc_handle
        = tokio::spawn(async move {
            ipc_server.run(push_tx).await;
        });

    let result
        = execute_resolved_tasks(project, resolved, &root_task, args, options, &socket_name, push_rx).await;

    ipc_handle.abort();

    result
}

pub fn task_exists(project: &Project, task_name: &str) -> bool {
    let Ok(task_name)
        = TaskName::new(task_name)
    else {
        return false;
    };

    let Ok(workspace)
        = project.active_workspace()
    else {
        return false;
    };

    let task_file_path
        = workspace.taskfile_path();

    if !task_file_path.fs_exists_blocking() {
        return false;
    }

    let Ok(task_file_content)
        = task_file_path.fs_read_text_blocking()
    else {
        return false;
    };

    let Ok(task_file)
        = parse(&task_file_content)
    else {
        return false;
    };

    task_file.tasks.contains_key(task_name.as_str())
}

struct DynamicExecutionState {
    resolved: RwLock<zpm_tasks::ResolvedTasks>,
    target_tasks: RwLock<HashSet<TaskId>>,
    original_targets: RwLock<HashSet<TaskId>>,
    completed: RwLock<HashSet<TaskId>>,
    script_finished: RwLock<HashSet<TaskId>>,
    subtasks: RwLock<HashMap<TaskId, HashSet<TaskId>>>,
    prepared_tasks: RwLock<BTreeMap<TaskId, PreparedTask>>,
    color_index: RwLock<usize>,
}

impl DynamicExecutionState {
    fn new(resolved: zpm_tasks::ResolvedTasks, root_task: TaskId) -> Self {
        let mut target_tasks
            = HashSet::new();

        target_tasks.insert(root_task.clone());

        let mut original_targets
            = HashSet::new();

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

    fn all_targets_completed(&self) -> bool {
        let targets
            = self.target_tasks.read().unwrap();

        let completed
            = self.completed.read().unwrap();

        targets.iter().all(|t| completed.contains(t))
    }

    fn is_task_fully_completed(&self, task_id: &TaskId) -> bool {
        let script_finished
            = self.script_finished.read().unwrap();

        if !script_finished.contains(task_id) {
            return false;
        }

        let subtasks
            = self.subtasks.read().unwrap();

        let completed
            = self.completed.read().unwrap();

        if let Some(task_subtasks) = subtasks.get(task_id) {
            task_subtasks.iter().all(|s| completed.contains(s))
        } else {
            true
        }
    }

    fn try_complete_task(&self, task_id: &TaskId) -> bool {
        if self.is_task_fully_completed(task_id) {
            let mut completed
                = self.completed.write().unwrap();

            completed.insert(task_id.clone());
            true
        } else {
            false
        }
    }

    fn add_pushed_task(&self, project: &Project, task_name: &str, parent_task_id: Option<&str>) -> Result<(TaskId, usize), Error> {
        let task_name
            = TaskName::new(task_name)
                .map_err(|_| Error::TaskNameParseError(task_name.to_string()))?;

        let workspace
            = project.active_workspace()?;

        let task_id
            = TaskId {
                workspace: workspace.name.clone(),
                task_name,
            };

        if let Some(parent_str) = parent_task_id {
            if let Some(parent_id) = self.parse_task_id(project, parent_str) {
                let mut subtasks
                    = self.subtasks.write().unwrap();

                subtasks
                    .entry(parent_id)
                    .or_default()
                    .insert(task_id.clone());
            }
        }

        {
            let completed
                = self.completed.read().unwrap();

            let targets
                = self.target_tasks.read().unwrap();

            if completed.contains(&task_id) || targets.contains(&task_id) {
                return Ok((task_id, 0));
            }
        }

        let new_resolved
            = project.resolve_task(&task_id)?;

        {
            let mut resolved
                = self.resolved.write().unwrap();

            for (tid, prereqs) in new_resolved.tasks {
                resolved.tasks.entry(tid).or_insert(prereqs);
            }

            for (ident, tf) in new_resolved.task_files {
                resolved.task_files.entry(ident).or_insert(tf);
            }
        }

        {
            let mut targets
                = self.target_tasks.write().unwrap();

            targets.insert(task_id.clone());
        }

        let new_task_count
            = self.prepare_new_tasks(project)?;

        Ok((task_id, new_task_count))
    }

    fn prepare_new_tasks(&self, project: &Project) -> Result<usize, Error> {
        let resolved
            = self.resolved.read().unwrap();

        let mut prepared
            = self.prepared_tasks.write().unwrap();

        let mut color_index
            = self.color_index.write().unwrap();

        let colors: Vec<&DataType>
            = prefix_colors().take(5).collect();

        let mut new_count
            = 0;

        for task_id in resolved.tasks.keys() {
            if prepared.contains_key(task_id) {
                continue;
            }

            let Some(task_file)
                = resolved.task_files.get(&task_id.workspace)
            else {
                continue;
            };

            let Some(task)
                = task_file.tasks.get(task_id.task_name.as_str())
            else {
                continue;
            };

            if task.script.is_empty() {
                continue;
            }

            let Ok(workspace)
                = project.workspace_by_ident(&task_id.workspace)
            else {
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
                = colors[*color_index % colors.len()];

            *color_index += 1;

            let prefix
                = color.colorize(&format!(
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

    fn parse_task_id(&self, project: &Project, task_id_str: &str) -> Option<TaskId> {
        let (workspace_str, task_name_str)
            = task_id_str.split_once(':')?;

        let task_name
            = TaskName::new(task_name_str).ok()?;

        let ident
            = zpm_primitives::Ident::new(workspace_str);

        let workspace
            = project.workspace_by_ident(&ident).ok()?;

        Some(TaskId {
            workspace: workspace.name.clone(),
            task_name,
        })
    }
}

async fn execute_resolved_tasks(
    project: &Project,
    resolved: zpm_tasks::ResolvedTasks,
    target_task: &TaskId,
    args: &[String],
    options: &SpawnedTaskOptions,
    socket_name: &str,
    push_rx: mpsc::Receiver<PushRequest>,
) -> Result<ExitStatus, Error> {
    if resolved.tasks.is_empty() {
        return Ok(ExitStatus::from_raw(0));
    }

    let state
        = Arc::new(DynamicExecutionState::new(resolved, target_task.clone()));

    state.prepare_new_tasks(project)?;

    let dependency_count
        = {
            let resolved
                = state.resolved.read().unwrap();

            let prepared
                = state.prepared_tasks.read().unwrap();

            resolved.tasks.keys()
                .filter(|t| *t != target_task && prepared.contains_key(*t))
                .count()
        };

    let show_progress
        = options.silent_dependencies && is_terminal() && dependency_count > 0;

    let mut progress_handle
        = if show_progress {
            let progress_state
                = Arc::new(ProgressState::new(dependency_count));

            let progress_state_clone
                = progress_state.clone();

            Some((
                start_progress(move |frame_idx| progress_state_clone.format_progress(frame_idx)),
                progress_state,
            ))
        } else {
            None
        };

    execute_tasks_impl(
        project,
        state,
        target_task,
        args,
        options,
        socket_name,
        push_rx,
        &mut progress_handle,
    ).await
}

async fn execute_tasks_impl(
    project: &Project,
    state: Arc<DynamicExecutionState>,
    root_task: &TaskId,
    args: &[String],
    options: &SpawnedTaskOptions,
    socket_name: &str,
    mut push_rx: mpsc::Receiver<PushRequest>,
    progress: &mut Option<(ProgressHandle, Arc<ProgressState>)>,
) -> Result<ExitStatus, Error> {
    use std::collections::HashMap;
    use tokio::task::JoinHandle;

    let is_first_printed
        = Arc::new(AtomicBool::new(true));

    let mut running_handles: HashMap<TaskId, JoinHandle<Result<ExitStatus, Error>>>
        = HashMap::new();

    loop {
        if state.all_targets_completed() {
            break;
        }

        while let Ok(request) = push_rx.try_recv() {
            let response
                = match state.add_pushed_task(project, &request.task_name, request.parent_task_id.as_deref()) {
                    Ok((_, new_count)) => {
                        if let Some((_, progress_state)) = progress.as_ref() {
                            progress_state.add_to_total(new_count);
                        }
                        PushResponse::Ok
                    }
                    Err(e) => PushResponse::Error(e.to_string()),
                };

            let _ = request.response_tx.send(response);
        }

        let ready_tasks: Vec<TaskId>
            = {
                let resolved
                    = state.resolved.read().unwrap();

                let completed
                    = state.completed.read().unwrap();

                let script_finished
                    = state.script_finished.read().unwrap();

                let running: HashSet<TaskId>
                    = running_handles.keys().cloned().collect();

                resolved
                    .tasks
                    .iter()
                    .filter(|(task_id, prerequisites)| {
                        !completed.contains(*task_id)
                            && !script_finished.contains(*task_id)
                            && !running.contains(*task_id)
                            && prerequisites.iter().all(|p| completed.contains(p))
                    })
                    .map(|(task_id, _)| task_id.clone())
                    .collect()
            };

        for task_id in ready_tasks {
            let original_targets
                = state.original_targets.read().unwrap();

            let is_target
                = original_targets.contains(&task_id);

            drop(original_targets);

            let task_args: Vec<String>
                = if &task_id == root_task { args.to_vec() } else { vec![] };

            let prepared_opt
                = {
                    let prepared_tasks
                        = state.prepared_tasks.read().unwrap();

                    prepared_tasks.get(&task_id).cloned()
                };

            if let Some(prepared) = prepared_opt {
                if is_target {
                    if let Some((handle, _)) = progress {
                        handle.stop();
                    }
                }

                let task_display_name
                    = format!(
                        "{}:{}",
                        task_id.workspace.to_file_string(),
                        task_id.task_name.as_str()
                    );

                if !is_target {
                    if let Some((_, state)) = progress.as_ref() {
                        state.add_task(&task_display_name);
                    }
                }

                let is_first
                    = is_first_printed.clone();

                let opts
                    = options.clone();

                let progress_state
                    = progress.as_ref().map(|(_, state)| state.clone());

                let socket
                    = socket_name.to_string();

                let task_id_str
                    = task_display_name.clone();

                let handle
                    = tokio::spawn(async move {
                        let result
                            = execute_prepared_task_with_ipc(&prepared, &task_args, is_first, &opts, is_target, &socket, &task_id_str).await;

                        if !is_target {
                            if let Some(state) = progress_state {
                                state.remove_task(&task_display_name);
                            }
                        }

                        result
                    });

                running_handles.insert(task_id, handle);
            } else {
                let mut completed
                    = state.completed.write().unwrap();

                completed.insert(task_id);
            }
        }

        if running_handles.is_empty() {
            if state.all_targets_completed() {
                break;
            }

            tokio::select! {
                Some(request) = push_rx.recv() => {
                    let response
                        = match state.add_pushed_task(project, &request.task_name, request.parent_task_id.as_deref()) {
                            Ok((_, new_count)) => {
                                if let Some((_, progress_state)) = progress.as_ref() {
                                    progress_state.add_to_total(new_count);
                                }
                                PushResponse::Ok
                            }
                            Err(e) => PushResponse::Error(e.to_string()),
                        };

                    let _ = request.response_tx.send(response);
                }
            }

            continue;
        }

        let completed_task: (TaskId, Result<ExitStatus, Error>);

        tokio::select! {
            Some(request) = push_rx.recv() => {
                let response
                    = match state.add_pushed_task(project, &request.task_name, request.parent_task_id.as_deref()) {
                        Ok((_, new_count)) => {
                            if let Some((_, progress_state)) = progress.as_ref() {
                                progress_state.add_to_total(new_count);
                            }
                            PushResponse::Ok
                        }
                        Err(e) => PushResponse::Error(e.to_string()),
                    };

                let _ = request.response_tx.send(response);
                continue;
            }

            result = async {
                use futures::future::select_all;
                let handles: Vec<_> = running_handles.iter_mut().collect();
                let task_ids: Vec<_> = handles.iter().map(|(id, _)| (*id).clone()).collect();
                let futures: Vec<_> = handles.into_iter().map(|(_, h)| Box::pin(async move { h.await })).collect();
                let (result, idx, _) = select_all(futures).await;
                (task_ids[idx].clone(), result)
            } => {
                let (task_id, join_result) = result;
                running_handles.remove(&task_id);

                match join_result {
                    Ok(task_result) => {
                        completed_task = (task_id, task_result);
                    }
                    Err(e) => {
                        if let Some((handle, _)) = progress {
                            handle.stop();
                        }
                        return Err(Error::TaskJoinError(e.to_string()));
                    }
                }
            }
        }

        {
            let (task_id, task_result) = completed_task;
            match task_result {
                Ok(status) if status.success() => {
                    {
                        let mut script_finished
                            = state.script_finished.write().unwrap();

                        script_finished.insert(task_id.clone());
                    }

                    state.try_complete_task(&task_id);

                    let parents_to_check: Vec<TaskId>
                        = {
                            let subtasks
                                = state.subtasks.read().unwrap();

                            subtasks
                                .iter()
                                .filter(|(_, children)| children.contains(&task_id))
                                .map(|(parent, _)| parent.clone())
                                .collect()
                        };

                    for parent in parents_to_check {
                        state.try_complete_task(&parent);
                    }
                }
                Ok(status) => {
                    if let Some((handle, _)) = progress {
                        handle.stop();
                    }
                    return Ok(status);
                }
                Err(e) => {
                    if let Some((handle, _)) = progress {
                        handle.stop();
                    }
                    return Err(e);
                }
            }
        }
    }

    Ok(ExitStatus::from_raw(0))
}

async fn execute_prepared_task_with_ipc(
    prepared: &PreparedTask,
    args: &[String],
    is_first_printed: Arc<AtomicBool>,
    options: &SpawnedTaskOptions,
    is_target: bool,
    socket_name: &str,
    task_id_str: &str,
) -> Result<ExitStatus, Error> {
    let mut prepared_with_ipc
        = prepared.clone();

    prepared_with_ipc.env.insert(
        IPC_SOCKET_ENV.to_string(),
        socket_name.to_string(),
    );

    prepared_with_ipc.env.insert(
        IPC_CURRENT_TASK_ENV.to_string(),
        task_id_str.to_string(),
    );

    execute_prepared_task(&prepared_with_ipc, args, is_first_printed, options, is_target).await
}

fn build_task_script(script: &str, args: &[String]) -> String {
    if args.is_empty() {
        script.to_string()
    } else {
        let escaped_args: Vec<String>
            = args.iter()
                .map(|a| shell_escape(a))
                .collect();

        format!("set -- {}; {}", escaped_args.join(" "), script)
    }
}

fn write_line(writer: &mut std::io::StdoutLock<'_>, prefix: &str, line: &str, verbose_level: u8) {
    if verbose_level >= 1 {
        writeln!(writer, "{}{}", prefix, line).ok();
    } else {
        writeln!(writer, "{}", line).ok();
    }
}

async fn execute_prepared_task(
    prepared: &PreparedTask,
    args: &[String],
    is_first_printed: Arc<AtomicBool>,
    options: &SpawnedTaskOptions,
    is_target: bool,
) -> Result<ExitStatus, Error> {
    let start
        = Instant::now();

    let mut env
        = ScriptEnvironment::new()?;

    for (key, value) in &prepared.env {
        env = env.with_env_variable(key, value);
    }

    let show_output
        = !options.silent_dependencies || is_target;

    let use_inherited_stdio
        = options.silent_dependencies && options.verbose_level == 0 && is_target;

    if use_inherited_stdio {
        execute_inherited(prepared, args, env).await
    } else if options.interlaced && show_output {
        execute_interlaced(prepared, args, env, start, is_first_printed, options).await
    } else {
        execute_buffered(prepared, args, env, start, is_first_printed, options, show_output).await
    }
}

async fn execute_inherited(
    prepared: &PreparedTask,
    args: &[String],
    env: ScriptEnvironment,
) -> Result<ExitStatus, Error> {
    let script
        = build_task_script(&prepared.script, args);

    let empty_args: [String; 0]
        = [];

    let status
        = env
            .with_cwd(prepared.cwd.clone())
            .run_script_inherited(&script, empty_args)
            .await?;

    Ok(status)
}

async fn execute_interlaced(
    prepared: &PreparedTask,
    args: &[String],
    env: ScriptEnvironment,
    start: Instant,
    _is_first_printed: Arc<AtomicBool>,
    options: &SpawnedTaskOptions,
) -> Result<ExitStatus, Error> {
    let script
        = build_task_script(&prepared.script, args);

    let empty_args: [String; 0]
        = [];

    let mut running
        = env
            .with_cwd(prepared.cwd.clone())
            .spawn_script(&script, empty_args)
            .await?;

    let child_stdout
        = running.child.stdout.take().expect("Failed to capture stdout");

    let child_stderr
        = running.child.stderr.take().expect("Failed to capture stderr");

    let mut stdout_reader
        = BufReader::new(child_stdout).lines();

    let mut stderr_reader
        = BufReader::new(child_stderr).lines();

    if options.verbose_level >= 2 {
        let mut writer
            = std::io::stdout().lock();

        write_line(&mut writer, &prepared.prefix, "Process started", options.verbose_level);
    }

    let prefix
        = prepared.prefix.clone();

    let verbose
        = options.verbose_level;

    loop {
        tokio::select! {
            line = stdout_reader.next_line() => {
                match line {
                    Ok(Some(line)) => {
                        let mut writer
                            = std::io::stdout().lock();

                        write_line(&mut writer, &prefix, &line, verbose);
                    }
                    Ok(None) => break,
                    Err(_) => break,
                }
            }
            line = stderr_reader.next_line() => {
                match line {
                    Ok(Some(line)) => {
                        let mut writer
                            = std::io::stdout().lock();

                        write_line(&mut writer, &prefix, &line, verbose);
                    }
                    Ok(None) => {}
                    Err(_) => {}
                }
            }
        }
    }

    while let Ok(Some(line)) = stderr_reader.next_line().await {
        let mut writer
            = std::io::stdout().lock();

        write_line(&mut writer, &prefix, &line, verbose);
    }

    let status
        = running.child.wait().await?;

    let duration
        = start.elapsed();

    if options.verbose_level >= 2 {
        let mut writer
            = std::io::stdout().lock();

        let status_string
            = match status.code() {
                Some(code) => format!("exit code {}", DataType::Number.colorize(&format!("{}", code))),
                None => "exit code unknown".to_string(),
            };

        if options.enable_timers {
            write_line(
                &mut writer,
                &prepared.prefix,
                &format!(
                    "Process exited ({}), completed in {}",
                    status_string,
                    Unit::duration(duration.as_secs_f64()).to_print_string()
                ),
                options.verbose_level,
            );
        } else {
            write_line(
                &mut writer,
                &prepared.prefix,
                &format!("Process exited ({})", status_string),
                options.verbose_level,
            );
        }
    }

    Ok(status)
}

async fn execute_buffered(
    prepared: &PreparedTask,
    args: &[String],
    env: ScriptEnvironment,
    start: Instant,
    is_first_printed: Arc<AtomicBool>,
    options: &SpawnedTaskOptions,
    show_output: bool,
) -> Result<ExitStatus, Error> {
    let script
        = build_task_script(&prepared.script, args);

    let empty_args: [String; 0]
        = [];

    let result
        = env
            .with_cwd(prepared.cwd.clone())
            .run_script(&script, empty_args)
            .await?;

    let output
        = result.output();

    let duration
        = start.elapsed();

    let is_failure_output
        = !show_output && !output.status.success();

    if show_output || is_failure_output {
        let verbose_level
            = if is_failure_output { 2 } else { options.verbose_level };

        let stdout
            = String::from_utf8_lossy(&output.stdout);

        let stderr
            = String::from_utf8_lossy(&output.stderr);

        let mut writer
            = std::io::stdout().lock();

        if verbose_level >= 2 && !is_first_printed.swap(false, Ordering::Relaxed) {
            writeln!(writer).ok();
        }

        if verbose_level >= 2 {
            write_line(&mut writer, &prepared.prefix, "Process started", verbose_level);
        }

        for line in stdout.lines() {
            write_line(&mut writer, &prepared.prefix, line, verbose_level);
        }

        for line in stderr.lines() {
            write_line(&mut writer, &prepared.prefix, line, verbose_level);
        }

        if verbose_level >= 2 {
            let status_string
                = match output.status.code() {
                    Some(code) => format!("exit code {}", DataType::Number.colorize(&format!("{}", code))),
                    None => "exit code unknown".to_string(),
                };

            if options.enable_timers {
                write_line(
                    &mut writer,
                    &prepared.prefix,
                    &format!(
                        "Process exited ({}), completed in {}",
                        status_string,
                        Unit::duration(duration.as_secs_f64()).to_print_string()
                    ),
                    verbose_level,
                );
            } else {
                write_line(
                    &mut writer,
                    &prepared.prefix,
                    &format!("Process exited ({})", status_string),
                    verbose_level,
                );
            }
        }
    }

    Ok(output.status)
}
