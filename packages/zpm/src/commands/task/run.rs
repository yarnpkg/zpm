use std::{collections::{BTreeMap, BTreeSet, HashSet}, io::Write, os::unix::process::ExitStatusExt, process::ExitStatus, sync::{atomic::{AtomicBool, AtomicUsize, Ordering}, Arc, Mutex}, time::Instant};

use clipanion::cli;
use futures::future::try_join_all;
use tokio::io::{AsyncBufReadExt, BufReader};
use zpm_tasks::{parse, TaskId, TaskName};
use zpm_utils::{is_terminal, shell_escape, start_progress, DataType, Path, ProgressHandle, ToFileString, ToHumanString, Unit};

use crate::{error::Error, project::Project, script::ScriptEnvironment};

#[derive(Clone)]
struct SpawnedTaskOptions {
    verbose_level: u8,
    interlaced: bool,
    enable_timers: bool,
    silent_dependencies: bool,
}

struct ProgressState {
    total: usize,
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
            total,
            completed: AtomicUsize::new(0),
            running_tasks: Mutex::new(BTreeSet::new()),
            gradient_frames,
        }
    }

    fn add_task(&self, task_name: &str) {
        self.running_tasks.lock().unwrap().insert(task_name.to_string());
    }

    fn remove_task(&self, task_name: &str) {
        self.running_tasks.lock().unwrap().remove(task_name);
        self.completed.fetch_add(1, Ordering::Relaxed);
    }

    fn format_progress(&self, frame_idx: usize) -> String {
        let completed
            = self.completed.load(Ordering::Relaxed);

        let running
            = self.running_tasks.lock().unwrap().len();

        let scheduled
            = self.total - running - completed;

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
#[cli::path("task", "run")]
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

    if !task_file_path.fs_exists() {
        return Err(Error::TaskFileNotFound(workspace.path.clone()));
    }

    let task_file_content
        = task_file_path.fs_read_text()?;

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

    execute_resolved_tasks(project, &resolved, &root_task, args, options).await
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

    if !task_file_path.fs_exists() {
        return false;
    }

    let Ok(task_file_content)
        = task_file_path.fs_read_text()
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

async fn execute_resolved_tasks(
    project: &Project,
    resolved: &zpm_tasks::ResolvedTasks,
    target_task: &TaskId,
    args: &[String],
    options: &SpawnedTaskOptions,
) -> Result<ExitStatus, Error> {
    if resolved.tasks.is_empty() {
        return Ok(ExitStatus::from_raw(0));
    }

    let prepared_tasks
        = prepare_all_tasks(project, resolved)?;

    let dependency_count
        = resolved.tasks.keys()
            .filter(|t| *t != target_task && prepared_tasks.contains_key(*t))
            .count();

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
        resolved,
        target_task,
        args,
        options,
        prepared_tasks,
        &mut progress_handle,
    ).await
}

async fn execute_tasks_impl(
    resolved: &zpm_tasks::ResolvedTasks,
    target_task: &TaskId,
    args: &[String],
    options: &SpawnedTaskOptions,
    prepared_tasks: BTreeMap<TaskId, PreparedTask>,
    progress: &mut Option<(ProgressHandle, Arc<ProgressState>)>,
) -> Result<ExitStatus, Error> {
    let mut completed: HashSet<TaskId>
        = HashSet::new();

    let is_first_printed
        = Arc::new(AtomicBool::new(true));

    while !completed.contains(target_task) {
        let ready_tasks: Vec<&TaskId>
            = resolved
                .tasks
                .iter()
                .filter(|(task_id, prerequisites)| {
                    !completed.contains(*task_id)
                        && prerequisites.iter().all(|p| completed.contains(p))
                })
                .map(|(task_id, _)| task_id)
                .collect();

        if ready_tasks.is_empty() {
            break;
        }

        let mut handles
            = Vec::with_capacity(ready_tasks.len());

        let mut task_ids
            = Vec::with_capacity(ready_tasks.len());

        for task_id in ready_tasks {
            let is_target
                = task_id == target_task;

            let task_args
                = if is_target { args } else { &[] };

            if let Some(prepared) = prepared_tasks.get(task_id) {
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

                let prepared
                    = prepared.clone();

                let args_vec: Vec<String>
                    = task_args.to_vec();

                let is_first
                    = is_first_printed.clone();

                let opts
                    = options.clone();

                let progress_state
                    = progress.as_ref().map(|(_, state)| state.clone());

                let handle
                    = tokio::spawn(async move {
                        let result
                            = execute_prepared_task(&prepared, &args_vec, is_first, &opts, is_target).await;

                        if !is_target {
                            if let Some(state) = progress_state {
                                state.remove_task(&task_display_name);
                            }
                        }

                        result
                    });

                handles.push(handle);
                task_ids.push(task_id.clone());
            } else {
                completed.insert(task_id.clone());
            }
        }

        if handles.is_empty() {
            continue;
        }

        let results
            = try_join_all(handles)
                .await
                .map_err(|e| Error::TaskJoinError(e.to_string()))?;

        for (tid, result) in task_ids.into_iter().zip(results) {
            let status: ExitStatus
                = result?;

            if !status.success() {
                if let Some((handle, _)) = progress {
                    handle.stop();
                }
                return Ok(status);
            }

            completed.insert(tid);
        }
    }

    Ok(ExitStatus::from_raw(0))
}

fn prepare_all_tasks(
    project: &Project,
    resolved: &zpm_tasks::ResolvedTasks,
) -> Result<BTreeMap<TaskId, PreparedTask>, Error> {
    let mut prepared
        = BTreeMap::new();

    let mut color_it
        = prefix_colors();

    for task_id in resolved.tasks.keys() {
        let task_file
            = resolved.task_files.get(&task_id.workspace).ok_or_else(|| {
                Error::TaskWorkspaceNotFound(task_id.workspace.clone())
            })?;

        let task
            = task_file.tasks.get(task_id.task_name.as_str()).ok_or_else(|| {
                Error::TaskNotFound {
                    workspace: task_id.workspace.clone(),
                    task_name: task_id.task_name.as_str().to_string(),
                }
            })?;

        if task.script.is_empty() {
            continue;
        }

        let workspace
            = project.workspace_by_ident(&task_id.workspace)?;

        let script
            = task.script.join("\n");

        let mut env
            = BTreeMap::new();

        env.insert(
            "npm_lifecycle_event".to_string(),
            task_id.task_name.as_str().to_string(),
        );

        let color
            = color_it.next().unwrap();

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
    }

    Ok(prepared)
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
