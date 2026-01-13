use std::{collections::{BTreeMap, BTreeSet}, io::{StdoutLock, Write}, process::{ExitCode, ExitStatus, Stdio}, sync::{Arc, atomic::{AtomicBool, Ordering}}, time::Instant};

use clipanion::{cli, prelude::*};
use futures::{StreamExt, stream::FuturesUnordered};
use itertools::Itertools;
use tokio::io::{AsyncBufReadExt, BufReader};
use zpm_macro_enum::zpm_enum;
use zpm_primitives::Ident;
use zpm_utils::{DataType, Path, ToFileString, ToHumanString, Unit};

use crate::{
    algos::scc_tarjan_pearce, commands::{PartialYarnCli, YarnCli}, error::Error, git_utils, project::{Project, Workspace}, workspace_glob::WorkspaceGlob
};

/// An SCC island containing workspaces that can be run together,
/// along with external dependencies that must complete first.
#[derive(Debug)]
pub struct TopologicalIsland {
    /// Workspaces in this SCC (can be run in any order within the island)
    pub idents: Vec<Ident>,
    /// Dependencies from other islands that must complete before this island can start
    pub depends_on: BTreeSet<Ident>,
}

/// Selection result that can either be a simple list or topologically ordered islands
pub enum Selection {
    List(Vec<Ident>),
    Topological(Vec<TopologicalIsland>),
}

#[zpm_enum]
#[derive(Debug)]
pub enum FollowedDependencies {
    #[literal("all")]
    All,

    #[literal("dev")]
    Dev,

    #[literal("prod")]
    Prod,
}

#[zpm_enum]
#[derive(Debug)]
#[derive_variants(Debug)]
pub enum Limit {
    #[pattern(r"^(?<limit>\d+)$")]
    #[to_file_string(|params| format!("{}", params.limit))]
    #[to_print_string(|params| format!("{}", params.limit))]
    Fixed {
        limit: usize,
    },

    #[literal("unlimited")]
    Unlimited,
}


#[cli::command(proxy)]
#[cli::path("workspaces", "foreach")]
pub struct WorkspacesForeach {
    #[cli::option("-A,--all", default = false)]
    all: bool,

    #[cli::option("--from", default = vec![])]
    from: Vec<WorkspaceGlob>,

    #[cli::option("--since")]
    since: Option<Option<String>>,

    #[cli::option("--recursive", default = false)]
    recursive: bool,

    #[cli::option("--follow-dependencies", default = false)]
    follow_dependencies: bool,

    #[cli::option("--follow-dependents", default = false)]
    follow_dependents: bool,

    #[cli::option("--followed-dependencies", default = FollowedDependencies::All)]
    followed_dependencies: FollowedDependencies,

    #[cli::option("--include", default = vec![])]
    include: Vec<WorkspaceGlob>,

    #[cli::option("--exclude", default = vec![])]
    exclude: Vec<WorkspaceGlob>,

    #[cli::option("-p,--parallel", default = false)]
    is_parallel: bool,

    #[cli::option("-i,--interlaced", default = false)]
    is_interlaced: bool,

    #[cli::option("-j,--jobs", default = FixedLimit {limit: 10}.into())]
    jobs: Limit,

    #[cli::option("--topological", default = false)]
    is_topological: bool,

    #[cli::option("-v,--verbose", default = if zpm_utils::is_terminal() {2} else {0}, counter)]
    verbose_level: u8,

    #[cli::option("--private", default = true)]
    private: bool,

    command: String,

    args: Vec<String>,
}

impl WorkspacesForeach {
    pub async fn execute(&self) -> Result<ExitCode, Error> {
        let mut project
            = Project::new(None).await?;

        project.lazy_install().await?;

        let mut args
            = vec![self.command.clone()];

        args.extend(self.args.clone());

        let selection
            = self.selection(&project, args.clone()).await?;

        match selection {
            Selection::List(idents) => {
                self.execute_list(&project, idents, args).await
            },

            Selection::Topological(islands) => {
                self.execute_topological(&project, islands, args).await
            },
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

    fn prefix_for_ident(&self, ident: &Ident, color: &DataType) -> String {
        color.colorize(&format!("[{}]: ", ident.to_file_string()))
    }

    fn jobs(&self) -> usize {
        if self.is_parallel {
            match &self.jobs {
                Limit::Fixed(params) => params.limit.max(1),
                Limit::Unlimited => usize::MAX,
            }
        } else {
            1
        }
    }

    fn is_interlaced(&self) -> bool {
        self.is_parallel && (self.is_interlaced || self.jobs() == 1)
    }

    fn follow_dependencies(&self) -> bool {
        self.follow_dependencies || (self.recursive && self.since.is_none())
    }

    fn follow_dependents(&self) -> bool {
        self.follow_dependents || (self.recursive && self.since.is_some())
    }

    fn print_epilogue(&self, project: &Project, start: Instant, task_count: usize) {
        if self.verbose_level >= 2 && project.config.settings.enable_timers.value {
            let duration
                = start.elapsed();

            println!();
            println!("Completed {} tasks in {}", DataType::Number.colorize(&format!("{}", task_count)), Unit::duration(duration.as_secs_f64()).to_print_string());
        }
    }

    async fn execute_list(&self, project: &Project, idents: Vec<Ident>, args: Vec<String>) -> Result<ExitCode, Error> {
        let mut color_it
            = Self::prefix_colors();

        let start_time
            = Instant::now();

        let task_count
            = idents.len();

        let mut futs: FuturesUnordered<tokio::task::JoinHandle<Result<ExitStatus, Error>>>
            = FuturesUnordered::new();

        let mut exit_code
            = ExitCode::SUCCESS;

        let is_first_printed_task
            = Arc::new(AtomicBool::new(true));

        for ident in idents {
            let task = Task {
                prefix: self.prefix_for_ident(&ident, color_it.next().unwrap()),

                cwd: project.workspace_by_ident(&ident)?.path.clone(),
                args: args.clone(),

                enable_timers: project.config.settings.enable_timers.value,
                verbose_level: self.verbose_level,
                is_interlaced: self.is_interlaced(),

                is_first_printed_task: is_first_printed_task.clone(),
            };

            if futs.len() == self.jobs() {
                if let Some(result) = futs.next().await {
                    if !result??.success() {
                        exit_code = ExitCode::FAILURE;
                    }
                }
            }

            let handle = tokio::spawn(async move {
                task.run().await
            });

            futs.push(handle);
        }

        while let Some(result) = futs.next().await {
            if !result??.success() {
                exit_code = ExitCode::FAILURE;
            }
        }

        self.print_epilogue(project, start_time, task_count);

        Ok(exit_code)
    }

    async fn execute_topological(&self, project: &Project, mut islands: Vec<TopologicalIsland>, args: Vec<String>) -> Result<ExitCode, Error> {
        let start_time
            = Instant::now();

        let task_count
            = islands.iter().map(|island| island.idents.len()).sum::<usize>();

        let mut color_it
            = Self::prefix_colors();

        let is_first_printed_task
            = Arc::new(AtomicBool::new(true));

        // Track completed idents
        let mut completed: BTreeSet<Ident>
            = BTreeSet::new();

        let mut in_flight: FuturesUnordered<tokio::task::JoinHandle<Result<_, Error>>>
            = FuturesUnordered::new();

        let mut processing: BTreeSet<Ident>
            = BTreeSet::new();

        let mut exit_code
            = ExitCode::SUCCESS;

        loop {
            while in_flight.len() < self.jobs() {
                let next_ident
                    = self.find_next_schedulable(&islands, &completed, &processing);

                let Some(ident)
                    = next_ident
                else {
                    break;
                };

                processing.insert(ident.clone());

                let task = Task {
                    prefix: self.prefix_for_ident(&ident, color_it.next().unwrap()),

                    cwd: project.workspace_by_ident(&ident)?.path.clone(),
                    args: args.clone(),

                    enable_timers: project.config.settings.enable_timers.value,
                    verbose_level: self.verbose_level,
                    is_interlaced: self.is_interlaced(),

                    is_first_printed_task: is_first_printed_task.clone(),
                };

                let handle = tokio::spawn(async move {
                    Ok((task.run().await?, ident))
                });

                in_flight.push(handle);
            }

            // If no jobs are in flight and we couldn't schedule any, we're done
            if in_flight.is_empty() {
                break;
            }

            // Wait for at least one job to complete
            if let Some(result) = in_flight.next().await {
                let (status, ident)
                    = result??;

                if status.success() {
                    processing.remove(&ident);
                    completed.insert(ident.clone());

                    // Remove completed ident from islands to speed up future searches
                    self.remove_completed_from_islands(&mut islands, &ident);
                } else {
                    exit_code = ExitCode::FAILURE;
                }
            }
        }

        self.print_epilogue(project, start_time, task_count);

        Ok(exit_code)
    }

    /// Find the next schedulable ident from ready islands.
    /// An island is ready when all its depends_on idents are completed.
    fn find_next_schedulable(&self, islands: &[TopologicalIsland], completed: &BTreeSet<Ident>, processing: &BTreeSet<Ident>) -> Option<Ident> {
        for island in islands {
            // Check if all dependencies are completed
            if !island.depends_on.iter().all(|dep| completed.contains(dep)) {
                continue;
            }

            // Find first ident in this island that's not yet processing or completed
            for ident in &island.idents {
                if !completed.contains(ident) && !processing.contains(ident) {
                    return Some(ident.clone());
                }
            }
        }

        None
    }

    /// Remove a completed ident from all islands to speed up future searches.
    fn remove_completed_from_islands(&self, islands: &mut Vec<TopologicalIsland>, completed_ident: &Ident) {
        for island in islands.iter_mut() {
            island.idents.retain(|i| i != completed_ident);
            island.depends_on.remove(completed_ident);
        }

        islands.retain(|island| {
            !island.idents.is_empty()
        });
    }

    async fn select_changed_workspaces(&self, project: &Project, since: Option<&str>) -> Result<BTreeSet<Ident>, Error> {
        let changed_workspaces
            = git_utils::fetch_changed_workspaces(&project, since).await?;

        Ok(changed_workspaces.keys().cloned().collect())
    }

    async fn selection(&self, project: &Project, args: Vec<String>) -> Result<Selection, Error> {
        let mut selection: BTreeSet<Ident> = if self.all {
            project.workspaces_by_ident.keys().cloned().collect()
        } else if self.from.len() > 0 {
            project.workspaces.iter().filter(|w| self.from.iter().any(|f| f.check(w))).map(|w| w.name.clone()).collect()
        } else if let Some(since) = &self.since {
            self.select_changed_workspaces(project, since.as_deref()).await?
        } else if self.follow_dependencies() || self.follow_dependents() {
            BTreeSet::from_iter([project.active_workspace()?.name.clone()])
        } else {
            return Err(Error::ReplaceMe);
        };

        let dependencies
            = self.select_dependencies(project, &selection)?;
        let dependents
            = self.select_dependents(project, &selection)?;

        selection.extend(dependencies);
        selection.extend(dependents);

        selection.retain(|ident| {
            let workspace
                = project.workspace_by_ident(ident)
                    .expect("We should only be selecting workspaces, so this should never fail");

            !self.should_exclude(workspace)
        });

        if let Some((script_name, binaries_only)) = self.script_name(args) {
            selection.retain(|ident| {
                let workspace
                    = project.workspace_by_ident(ident)
                        .expect("We should only be selecting workspaces, so this should never fail");

                let locator
                    = workspace.locator();

                if project.package_visible_binaries(&locator).map_or(false, |map| map.contains_key(&script_name)) {
                    return true;
                }

                if !binaries_only && project.find_package_script(&locator, &script_name).is_ok() {
                    if std::env::var("npm_lifecycle_event") == Ok(script_name.clone()) && project.active_package().ok() == Some(locator) {
                        return false;
                    }

                    return true;
                }

                false
            });
        }

        if self.is_topological {
            Ok(Selection::Topological(self.topological_sort(project, &selection)))
        } else {
            Ok(Selection::List(selection.into_iter().collect()))
        }
    }

    fn topological_sort(&self, project: &Project, selection: &BTreeSet<Ident>) -> Vec<TopologicalIsland> {
        // Build dependency graph for selected workspaces only
        let mut graph: BTreeMap<Ident, BTreeSet<Ident>>
            = BTreeMap::new();

        for ident in selection {
            let workspace
                = project.workspace_by_ident(ident)
                    .expect("We should only be selecting workspaces");

            // Get dependencies that are also in the selection
            let deps: BTreeSet<Ident> = self.followed_dependencies(workspace)
                .into_iter()
                .filter(|dep| selection.contains(dep))
                .collect();

            graph.insert(ident.clone(), deps);
        }

        // SCC returns components in reverse topological order (dependencies first)
        let sccs
            = scc_tarjan_pearce(&graph);

        // Build islands with their external dependencies
        sccs.into_iter()
            .map(|scc_idents| {
                let scc_set: BTreeSet<Ident>
                    = scc_idents.iter().cloned().collect();

                // External dependencies are deps that are not in this SCC
                let mut depends_on: BTreeSet<Ident>
                    = BTreeSet::new();

                for ident in &scc_idents {
                    if let Some(deps) = graph.get(ident) {
                        for dep in deps {
                            // Only include if it's external to this SCC
                            if !scc_set.contains(dep) {
                                depends_on.insert(dep.clone());
                            }
                        }
                    }
                }

                TopologicalIsland {
                    idents: scc_idents,
                    depends_on,
                }
            })
            .collect()
    }

    fn script_name(&self, args: Vec<String>) -> Option<(String, bool)> {
        let builder
            = YarnCli::build_cli()
                .expect("Expected the CLI to be available since we're running in a command");

        let nested_environment
            = self.cli_environment.clone()
                .with_argv(args);

        let cli_parse
            = YarnCli::parse_args(&builder, &nested_environment);

        if let Ok(clipanion::core::SelectionResult::Command(_, _, PartialYarnCli::Run(params))) = cli_parse {
            let binaries_only
                = params.binaries_only
                    .unwrap_or(false);

            if let Some(name) = params.name {
                return Some((name.clone(), binaries_only));
            }
        }

        None
    }

    fn should_exclude(&self, workspace: &Workspace) -> bool {
        if !self.private && workspace.manifest.private == Some(true) {
            return true;
        }

        if self.include.len() > 0 {
            if !self.include.iter().any(|include| include.check(workspace)) {
                return true;
            }
        }

        self.exclude.iter().any(|exclude| {
            exclude.check(workspace)
        })
    }

    fn followed_dependencies(&self, workspace: &Workspace) -> BTreeSet<Ident> {
        let mut dependencies
            = BTreeSet::new();

        if matches!(self.followed_dependencies, FollowedDependencies::All | FollowedDependencies::Prod) {
            for (ident, descriptor) in workspace.manifest.remote.dependencies.iter() {
                if descriptor.range.is_workspace() {
                    dependencies.insert(ident.clone());
                }
            }
        }

        if matches!(self.followed_dependencies, FollowedDependencies::All | FollowedDependencies::Dev) {
            for (ident, descriptor) in workspace.manifest.dev_dependencies.iter() {
                if descriptor.range.is_workspace() {
                    dependencies.insert(ident.clone());
                }
            }
        }

        dependencies
    }

    fn select_dependencies(&self, project: &Project, selection: &BTreeSet<Ident>) -> Result<BTreeSet<Ident>, Error> {
        if !self.follow_dependencies() {
            return Ok(BTreeSet::new());
        }

        let mut seen
            = selection.clone();

        let mut queue
            = selection.iter().cloned().collect_vec();

        while let Some(last) = queue.pop() {
            let workspace
                = project.workspace_by_ident(&last)?;

            for dependency in self.followed_dependencies(workspace) {
                if seen.insert(dependency.clone()) {
                    queue.push(dependency);
                }
            }
        }

        Ok(seen)
    }

    fn build_dependent_map(&self, project: &Project) -> Result<BTreeMap<Ident, BTreeSet<Ident>>, Error> {
        if !self.follow_dependents() {
            return Ok(BTreeMap::new());
        }

        let mut dependent_map
            = BTreeMap::new();

        for workspace in project.workspaces.iter() {
            for dependency in self.followed_dependencies(workspace) {
                dependent_map.entry(dependency)
                    .or_insert_with(BTreeSet::new)
                    .insert(workspace.name.clone());
            }
        }

        Ok(dependent_map)
    }

    fn select_dependents(&self, project: &Project, selection: &BTreeSet<Ident>) -> Result<BTreeSet<Ident>, Error> {
        let dependent_map
            = self.build_dependent_map(project)?;

        let mut seen
            = selection.clone();

        let mut queue
            = selection.iter().collect_vec();

        while let Some(last) = queue.pop() {
            if let Some(dependents) = dependent_map.get(&last) {
                for dependent in dependents {
                    if seen.insert(dependent.clone()) {
                        queue.push(dependent);
                    }
                }
            }
        }

        Ok(seen)
    }
}

struct Task {
    pub prefix: String,

    pub cwd: Path,
    pub args: Vec<String>,

    pub enable_timers: bool,
    pub is_interlaced: bool,
    pub verbose_level: u8,

    pub is_first_printed_task: Arc<AtomicBool>,
}

impl Task {
    fn write_ln(&mut self, writer: &mut StdoutLock<'_>, str: &str) {
        if self.verbose_level >= 1 {
            writeln!(writer, "{}{}", self.prefix, str).unwrap();
        } else {
            writeln!(writer, "{}", str).unwrap();
        }
    }

    fn write_prologue(&mut self, writer: &mut StdoutLock<'_>) {
        if self.verbose_level >= 2 {
            self.write_ln(writer, "Process started");
        }
    }

    fn write_epilogue(&mut self, writer: &mut StdoutLock<'_>, start: Instant, status: ExitStatus) -> Result<ExitStatus, Error> {
        let duration
            = start.elapsed();

        let status_string = match status.code() {
            Some(code) => format!("exit code {}", DataType::Number.colorize(&format!("{}", code))),
            None => "exit code unknown".to_string(),
        };

        if self.verbose_level >= 2 {
            if self.enable_timers {
                self.write_ln(writer, &format!("Process exited ({status_string}), completed in {duration}", duration = Unit::duration(duration.as_secs_f64()).to_print_string()));
            } else {
                self.write_ln(writer, &format!("Process exited ({status_string})"));
            }
        }

        Ok(status)
    }

    pub async fn run(mut self) -> Result<ExitStatus, Error> {
        let start
            = Instant::now();

        let mut child
            = tokio::process::Command::new(Path::current_exe().unwrap().to_path_buf())
                .args(self.args.clone())
                .current_dir(self.cwd.to_path_buf())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()?;

        let child_stdout
            = child.stdout.take()
                .expect("Child did not have a handle to stdout");

        let mut child_reader
            = BufReader::new(child_stdout).lines();

        if self.is_interlaced {
            self.write_prologue(&mut std::io::stdout().lock());

            while let Some(line) = child_reader.next_line().await? {
                self.write_ln(&mut std::io::stdout().lock(), &line);
            }

            let status
                = child.wait().await?;

            self.write_epilogue(&mut std::io::stdout().lock(), start, status)
        } else {
            let mut lines
                = Vec::new();

            while let Some(line) = child_reader.next_line().await? {
                lines.push(line);
            }

            let status
                = child.wait().await?;

            let mut writer
                = std::io::stdout().lock();

            if self.verbose_level >= 2 && !self.is_first_printed_task.swap(false, Ordering::Relaxed) {
                println!();
            }

            self.write_prologue(&mut writer);

            for line in lines {
                self.write_ln(&mut writer, &line);
            }

            self.write_epilogue(&mut writer, start, status)
        }
    }
}
