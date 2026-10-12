use std::process::ExitStatus;

use clipanion::cli;

use super::run_buffered::BufferedHandler;
use super::run_errors_only::ErrorsOnlyHandler;
use super::run_interlaced::InterlacedHandler;
use super::run_silent_dependencies::SilentDependenciesHandler;
use super::runner::{run_task, run_tasks, TaskRunOptions};
use crate::daemon::DAEMON_SERVER_ENV_NAME;
use crate::error::Error;
use crate::workspace_glob::WorkspaceGlob;
use crate::workspace_selection::{FollowedDependencies, WorkspaceSelection};

/// Run a task
///
/// This command runs a task (and its dependencies, in the right order) from
/// the `taskfile` of the current workspace, or across many workspaces at once
/// when a workspace selection option is set.
///
/// Workspace selection uses the same options as `yarn workspaces foreach`:
///
/// - `-A,--all` runs the task in every workspace declaring it.
///
/// - `--from <glob>` runs it in the workspaces matching the glob (ident glob
///   like `@scope/*` or path glob like `./packages/*`); can be repeated.
///
/// - `--since [ref]` runs it in the workspaces changed since the ref, or since
///   the configured base refs (`changesetBaseRefs`) when no ref is provided.
///
/// - `--follow-dependencies` adds the transitive workspace dependencies of the
///   selection, and `--follow-dependents` adds its dependents. `--recursive`
///   follows dependents when combined with `--since` (so `--since --recursive`
///   selects all the affected workspaces), and dependencies otherwise.
///   `--followed-dependencies` restricts the traversal to `prod` or `dev`
///   dependencies. Without `-A`/`--from`/`--since`, they apply to the current
///   workspace.
///
/// - `--include`/`--exclude <glob>` and `--no-private` filter the final
///   selection.
///
/// All selected tasks are resolved into a single deduplicated graph. Use
/// `--only` to skip cross-workspace dependencies (e.g. `^build`) pointing
/// outside of the selection. When running across workspaces the first
/// failure cancels the run unless `--continue` is set.
///
/// The daemon limits the number of processes running at once across all
/// runs, according to the `taskConcurrency` setting (CPU count by default).
///
/// Several tasks can be run at once by separating them with commas
/// (`yarn tasks run build,typecheck -A`).
///
/// Output modes: interlaced (default), `--buffered`, `--silent-dependencies`,
/// `--errors-only` (only print the logs of failed tasks), and `--json`.
///
/// By default the tasks run through the background daemon; on CI (when the
/// `CI` environment variable is set) they run in an in-process daemon instead
/// (`--standalone`), which can be toggled with `--standalone`/`--no-standalone`.
/// Runs started from within a task always reuse the daemon running it.
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

    /// Run the task in the workspaces changed since a ref, or since the configured base refs when no ref is provided
    #[cli::option("--since")]
    since: Option<Option<String>>,

    /// Follow dependencies, or dependents when combined with `--since`
    #[cli::option("--recursive", default = false)]
    recursive: bool,

    /// Also run the task in the dependencies of the selected workspaces
    #[cli::option("--follow-dependencies", default = false)]
    follow_dependencies: bool,

    /// Also run the task in the dependents of the selected workspaces
    #[cli::option("--follow-dependents", default = false)]
    follow_dependents: bool,

    /// Dependency kinds followed by `--follow-dependencies` and `--follow-dependents`
    #[cli::option("--followed-dependencies", default = FollowedDependencies::All)]
    followed_dependencies: FollowedDependencies,

    /// Only keep workspaces matching these patterns
    #[cli::option("--include", default = vec![])]
    include: Vec<WorkspaceGlob>,

    /// Skip workspaces matching these patterns
    #[cli::option("--exclude", default = vec![])]
    exclude: Vec<WorkspaceGlob>,

    /// Include private workspaces
    #[cli::option("--private", default = true)]
    private: bool,

    /// Skip cross-workspace task dependencies outside of the selection
    #[cli::option("--only", default = false)]
    only: bool,

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

    /// Name of the task to run (comma-separated for multiple tasks)
    name: String,

    /// Arguments to pass to the task
    args: Vec<String>,
}

impl TaskRun {
    fn selection(&self) -> Option<WorkspaceSelection<'_>> {
        let selection = WorkspaceSelection {
            all: self.all,
            from: &self.from,
            since: &self.since,
            recursive: self.recursive,
            follow_dependencies: self.follow_dependencies,
            follow_dependents: self.follow_dependents,
            followed_dependencies: &self.followed_dependencies,
            include: &self.include,
            exclude: &self.exclude,
            private: self.private,
        };

        selection.is_active().then_some(selection)
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
            = selection.is_some() || names.len() > 1;

        // Multi-workspace output without prefixes would be unreadable
        let verbose_level = match is_multi && !self.json {
            true => self.verbose_level.max(1),
            false => self.verbose_level,
        };

        let options = TaskRunOptions {
            selection,
            // Nested runs reuse the daemon running their parent task
            standalone: self.standalone.unwrap_or_else(|| zpm_utils::is_ci().is_some() && std::env::var(DAEMON_SERVER_ENV_NAME).is_err()),
            verbose_level,
            only: self.only,
            continue_on_error: self.continue_on_error,
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
