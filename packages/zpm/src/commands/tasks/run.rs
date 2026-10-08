use std::process::ExitStatus;

use clipanion::cli;

use super::run_buffered::BufferedHandler;
use super::run_errors_only::ErrorsOnlyHandler;
use super::run_interlaced::InterlacedHandler;
use super::run_silent_dependencies::SilentDependenciesHandler;
use super::runner::{run_task, run_tasks, TaskRunOptions};
use super::selection::TaskSelection;
use crate::error::Error;
use crate::workspace_glob::WorkspaceGlob;

/// Run a task
///
/// This command runs a task (and its dependencies, in the right order) from
/// the `taskfile` of the current workspace, or across many workspaces at once
/// when a workspace selection option is set.
///
/// Workspace selection (turbo `--filter` equivalents):
///
/// - `-A,--all` runs the task in every workspace declaring it.
///
/// - `--from <glob>` runs it in the workspaces matching the glob (ident glob
///   like `@scope/*` or path glob like `./packages/*`); can be repeated.
///
/// - `--affected` runs it in the workspaces changed since the configured base
///   refs (`changesetBaseRefs`) and in all the workspaces depending on them
///   (`turbo run --affected`); `--since <ref>` does the same against an
///   explicit ref.
///
/// - `--with-dependencies` (alias `--recursive`) adds the transitive workspace
///   dependencies of the selection (`--filter pkg...`), `--dependencies-only`
///   replaces the selection by its dependencies (`--filter pkg^...`), and
///   `--with-dependents` adds its dependents (`--filter ...pkg`). When
///   combined with `--with-dependencies`, dependencies are followed from the
///   dependents too (`--filter ...pkg...`). Without `-A`/`--from`/`--since`,
///   they apply to the current workspace.
///
/// - `--include`/`--exclude <glob>` filter the final selection.
///
/// All selected tasks are resolved into a single deduplicated graph. Use
/// `--only` to skip cross-workspace dependencies (e.g. `^build`) pointing
/// outside of the selection. When running across workspaces the first
/// failure cancels the run unless `--continue` is set, and at most
/// `-j,--concurrency` processes (CPU count by default) run at once.
///
/// Several tasks can be run at once by separating them with commas
/// (`yarn tasks run build,typecheck -A`).
///
/// Output modes: interlaced (default), `--buffered`, `--silent-dependencies`,
/// `--errors-only` (only print the logs of failed tasks), and `--json`.
///
/// Tasks declaring `@cache` are restored from the task cache when their
/// inputs didn't change; `--no-cache` (alias `--force`) ignores the existing
/// entries (successful runs are still stored).
///
/// By default the tasks run through the background daemon; on CI (when the
/// `CI` environment variable is set) they run in an in-process daemon instead
/// (`--standalone`), which can be toggled with `--standalone`/`--no-standalone`.
#[cli::command(proxy)]
#[cli::path("tasks", "run")]
#[cli::category("Task management commands")]
pub struct TaskRun {
    /// Run the task in every workspace declaring it
    #[cli::option("-A,--all", default = false)]
    all: bool,

    /// Run the task in the workspaces matching these patterns
    #[cli::option("--from", default = vec![])]
    from: Vec<WorkspaceGlob>,

    /// Run the task in the workspaces changed since a ref (and their dependents)
    #[cli::option("--since")]
    since: Option<String>,

    /// Run the task in the workspaces changed since the base refs (and their dependents)
    #[cli::option("--affected", default = false)]
    affected: bool,

    /// Also run the task in the dependencies of the selected workspaces
    #[cli::option("--with-dependencies,--recursive", default = false)]
    with_dependencies: bool,

    /// Run the task in the dependencies of the selected workspaces, but not in the workspaces themselves
    #[cli::option("--dependencies-only", default = false)]
    dependencies_only: bool,

    /// Also run the task in the dependents of the selected workspaces
    #[cli::option("--with-dependents", default = false)]
    with_dependents: bool,

    /// Only keep workspaces matching these patterns
    #[cli::option("--include", default = vec![])]
    include: Vec<WorkspaceGlob>,

    /// Skip workspaces matching these patterns
    #[cli::option("--exclude", default = vec![])]
    exclude: Vec<WorkspaceGlob>,

    /// Skip cross-workspace task dependencies outside of the selection
    #[cli::option("--only", default = false)]
    only: bool,

    /// Maximum number of processes running at once (defaults to the CPU count for multi-workspace runs)
    #[cli::option("-j,--concurrency")]
    concurrency: Option<usize>,

    /// Keep running independent tasks after a failure
    #[cli::option("--continue", default = false)]
    continue_on_error: bool,

    /// Print the output of each task once it completes
    #[cli::option("--buffered", default = false)]
    buffered: bool,

    /// Only print the output of the target tasks
    #[cli::option("--silent-dependencies", default = false)]
    silent_dependencies: bool,

    /// Only print the output of the tasks that failed
    #[cli::option("--errors-only", default = false)]
    errors_only: bool,

    /// Prefix each output line with a timestamp
    #[cli::option("--timestamps", default = false)]
    timestamps: bool,

    /// Output JSON objects (one per line) for each task event
    #[cli::option("--json", default = false)]
    json: bool,

    /// Increase the verbosity level (can be repeated)
    #[cli::option("-v,--verbose", default = if zpm_utils::is_terminal() {2} else {0}, counter)]
    verbose_level: u8,

    /// Run the tasks in an in-process daemon rather than the background one
    #[cli::option("--standalone")]
    standalone: Option<bool>,

    /// Ignore the task cache entries (results of successful runs are still stored)
    #[cli::option("--no-cache,--force", default = false)]
    no_cache: bool,

    /// Name of the task to run (comma-separated for multiple tasks)
    name: String,

    /// Arguments to pass to the task
    args: Vec<String>,
}

impl TaskRun {
    fn selection(&self) -> TaskSelection<'_> {
        TaskSelection {
            all: self.all,
            from: &self.from,
            since: match (&self.since, self.affected) {
                (Some(since), _) => Some(Some(since.clone())),
                (None, true) => Some(None),
                (None, false) => None,
            },
            with_dependencies: self.with_dependencies,
            dependencies_only: self.dependencies_only,
            with_dependents: self.with_dependents,
            include: &self.include,
            exclude: &self.exclude,
        }
    }

    pub async fn execute(&self) -> Result<ExitStatus, Error> {
        let names: Vec<String>
            = self.name.split(',')
                .map(|name| name.trim().to_string())
                .filter(|name| !name.is_empty())
                .collect();

        let selection
            = self.selection();

        let is_multi
            = selection.is_active() || names.len() > 1;

        let concurrency = match (self.concurrency, is_multi) {
            (Some(concurrency), _) => Some(concurrency.max(1)),
            (None, true) => Some(std::thread::available_parallelism().map_or(4, |n| n.get())),
            (None, false) => None,
        };

        // Multi-workspace output without prefixes would be unreadable
        let verbose_level = match is_multi && !self.json {
            true => self.verbose_level.max(1),
            false => self.verbose_level,
        };

        let options = TaskRunOptions {
            selection,
            standalone: self.standalone.unwrap_or_else(|| zpm_utils::is_ci().is_some()),
            verbose_level,
            only: self.only,
            concurrency,
            continue_on_error: self.continue_on_error,
            no_cache: self.no_cache,
        };

        if self.errors_only {
            let mut handler = ErrorsOnlyHandler {
                succeeded: 0,
                failed: vec![],
                cancelled: 0,
            };

            return run_tasks(&mut handler, &names, &self.args, &options).await;
        }

        if self.buffered {
            return run_tasks(&mut BufferedHandler, &names, &self.args, &options).await;
        }

        if self.silent_dependencies {
            let mut handler = SilentDependenciesHandler {
                progress_handle: None,
            };

            return run_tasks(&mut handler, &names, &self.args, &options).await;
        }

        let mut handler = InterlacedHandler {
            timestamps: self.timestamps,
            json: self.json,
        };

        run_tasks(&mut handler, &names, &self.args, &options).await
    }
}

/// Run a task of the active workspace with silent dependencies; used by
/// `yarn run <name>` when no script matches but a task does.
pub async fn run_silent_dependencies(name: &str, args: &[String]) -> Result<ExitStatus, Error> {
    let mut handler = SilentDependenciesHandler {
        progress_handle: None,
    };

    run_task(&mut handler, name, args, false, 0).await
}
