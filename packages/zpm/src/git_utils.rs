use std::collections::{BTreeMap, BTreeSet};

use itertools::Itertools;
use zpm_parsers::JsonDocument;
use zpm_primitives::Ident;
use zpm_utils::Path;

use crate::{error::Error, lockfile::Lockfile, lockfile_tree::find_changed_workspaces, project::{Project, LOCKFILE_NAME}, script::ScriptEnvironment};

pub fn find_root(initial_cwd: &Path) -> Result<Path, Error> {
    // Note: We can't just use `git rev-parse --show-toplevel`, because on Windows
    // it may return long paths even when the cwd uses short paths.

    for parent in initial_cwd.iter_path().rev() {
        let git_path = parent
            .with_join_str(".git");

        if git_path.fs_exists() {
            return Ok(parent);
        }
    }

    Err(Error::NoGitRoot)
}

pub async fn get_commit_title(root: &Path, hash: &str) -> Result<String, Error> {
    let title = ScriptEnvironment::new()?
        .with_cwd(root.clone())
        .run_exec("git", ["show", "--quiet", "--pretty=format:%s", hash])
        .await?
        .ok()?
        .stdout_text()?;

    Ok(title)
}

pub async fn get_commit_hash(target: &Path, hash: &str) -> Result<String, Error> {
    let mut env
        = ScriptEnvironment::new()?
            .with_cwd(target.clone());

    let result = env
        .run_exec("git", ["rev-parse", "--short", hash]).await?
        .ok()?
        .stdout_text()?;

    Ok(result)
}

pub async fn fetch_remotes(root: &Path) -> Result<Vec<String>, Error> {
    let result = ScriptEnvironment::new()?
        .with_cwd(root.clone())
        .run_exec("git", ["remote"])
        .await?
        .ok()?
        .stdout_text()?;

    let remotes = result
        .lines()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect();

    Ok(remotes)
}

pub async fn fetch_branch_base(project: &Project) -> Result<String, Error> {
    fetch_branch_base_of(project, "HEAD").await
}

/// The merge base between `head` and the first of the `changesetBaseRefs`
/// branches (or their remote counterparts) that exists.
pub async fn fetch_branch_base_of(project: &Project, head: &str) -> Result<String, Error> {
    let base_refs
        = project.config.settings.changeset_base_refs.iter()
            .map(|s| s.value.to_string())
            .collect_vec();

    let remotes
        = fetch_remotes(&project.project_cwd).await?;

    let mut branches
        = base_refs.clone();

    for remote in &remotes {
        for base_ref in &base_refs {
            branches.push(format!("{}/{}", remote, base_ref));
        }
    }

    loop {
        if branches.is_empty() {
            return Err(Error::NoMergeBaseFound(base_refs));
        }

        let mut args
            = vec!["merge-base".to_string(), head.to_string()];

        args.extend(branches.clone());

        let result = ScriptEnvironment::new()?
            .with_cwd(project.project_cwd.clone())
            .with_env_variable("LANG", "en_US")
            .run_exec("git", &args)
            .await?;

        if result.success() {
            return Ok(result.stdout_text()?);
        }

        let output
            = result.output();
        let stderr
            = String::from_utf8_lossy(&output.stderr);

        if let Some(invalid_branch) = parse_invalid_object_name(&stderr) {
            branches.retain(|b| b != &invalid_branch);
        } else {
            return Err(Error::NoMergeBaseFound(base_refs.clone()));
        }
    }
}

fn parse_invalid_object_name(stderr: &str) -> Option<String> {
    for line in stderr.lines() {
        let line
            = line.trim();

        for prefix in ["fatal: Not a valid object name ", "error: Not a valid object name "] {
            if let Some(rest) = line.strip_prefix(prefix) {
                return Some(rest.trim().to_string());
            }
        }
    }

    None
}

pub async fn fetch_base(root: &Path, base_refs: &[&str]) -> Result<String, Error> {
    let mut ancestor_bases
        = Vec::new();

    for &candidate in base_refs {
        let code = ScriptEnvironment::new()?
            .with_cwd(root.clone())
            .run_exec("git", ["merge-base", candidate, "HEAD"])
            .await?;

        if code.success() {
            ancestor_bases.push(candidate);
        }
    }

    if ancestor_bases.is_empty() {
        let base_refs = base_refs.iter()
            .map(|s| s.to_string())
            .collect();

        return Err(Error::NoMergeBaseFound(base_refs));
    }

    let merge_base_args = ["merge-base", "HEAD"].iter()
        .chain(ancestor_bases.iter())
        .collect::<Vec<_>>();

    let merge_base = ScriptEnvironment::new()?
        .with_cwd(root.clone())
        .run_exec("git", merge_base_args)
        .await?
        .ok()?
        .stdout_text()?;

    Ok(merge_base)
}

pub async fn fetch_changed_workspaces(project: &Project, since: Option<&str>) -> Result<BTreeMap<Ident, BTreeSet<Path>>, Error> {
    let since_ref = match since {
        Some(since) => since.to_string(),
        None => fetch_branch_base(project).await?,
    };

    let changed_files
        = fetch_changed_files_between(project, &since_ref, None).await?;

    changed_workspaces_from_files(project, &since_ref, None, &changed_files).await
}

/// Map changed files to the workspaces containing them; when the lockfile
/// changed, the workspaces whose dependency tree changed are added too.
async fn changed_workspaces_from_files(project: &Project, since_ref: &str, head: Option<&str>, changed_files: &BTreeSet<Path>) -> Result<BTreeMap<Ident, BTreeSet<Path>>, Error> {

    let mut changed_workspaces: BTreeMap<_, BTreeSet<_>>
        = BTreeMap::new();

    let lockfile_path
        = project.project_cwd.with_join_str(LOCKFILE_NAME);

    let lockfile_changed
        = changed_files.contains(&lockfile_path);

    for file in changed_files {
        // Skip the lockfile itself - we handle it separately via hash comparison
        if file == &lockfile_path {
            continue;
        }

        let workspace
            = project.workspaces.iter()
                .filter(|w| w.path.contains(file))
                .max_by_key(|w| w.path.as_str().len());

        if let Some(workspace) = workspace {
            let entry
                = changed_workspaces.entry(workspace.name.clone())
                    .or_default();

            entry.insert(file.clone());
        }
    }

    // If the lockfile changed, compare the dependency trees to find affected workspaces
    if lockfile_changed {
        // If we can't make sense of the lockfile from the working tree then
        // nothing else will; better to report it than to silently skip the
        // workspaces whose dependencies changed.
        let current_lockfile = match head {
            Some(head) => fetch_lockfile_at_ref(project, head).await?,
            None => project.lockfile()?,
        };

        // A base we can't read (the lockfile may simply not have existed
        // back then) can't vouch for anything; an empty lockfile makes all
        // the workspaces count as changed, which is the safe answer.
        let old_lockfile
            = fetch_lockfile_at_ref(project, since_ref).await
                .unwrap_or_else(|_| Lockfile::new());

        for ident in find_changed_workspaces(project, &old_lockfile, &current_lockfile) {
            changed_workspaces.entry(ident)
                .or_default()
                .insert(lockfile_path.clone());
        }
    }

    Ok(changed_workspaces)
}

/// Fetches and parses the lockfile at a specific git ref.
async fn fetch_lockfile_at_ref(project: &Project, git_ref: &str) -> Result<Lockfile, Error> {
    let lockfile_content
        = ScriptEnvironment::new()?
            .with_cwd(project.project_cwd.clone())
            // The leading `./` makes the path relative to the cwd rather than
            // to the repository root
            .run_exec("git", ["show", &format!("{}:./{}", git_ref, LOCKFILE_NAME)])
            .await?
            .ok()?
            .stdout_text()?;

    if lockfile_content.is_empty() {
        return Ok(Lockfile::new());
    }

    // Legacy Berry lockfiles start with '#'
    if lockfile_content.starts_with('#') {
        return Ok(Lockfile::new());
    }

    let lockfile: Lockfile
        = JsonDocument::hydrate_from_str(&lockfile_content)
            .map_err(|e| Error::LockfileParseError(e))?;

    Ok(lockfile)
}

pub async fn fetch_changed_files(project: &Project, since: Option<&str>) -> Result<BTreeSet<Path>, Error> {
    let since = match since {
        Some(since) => since.to_string(),
        None => fetch_branch_base(project).await?,
    };

    fetch_changed_files_between(project, &since, None).await
}

/// Files changed between `base` and `head`. Without `head` the comparison
/// is made against the working tree (including untracked files); with a
/// `head` ref only the committed changes between the two refs are listed
/// (same as `TURBO_SCM_BASE`/`TURBO_SCM_HEAD`).
pub async fn fetch_changed_files_between(project: &Project, since: &str, head: Option<&str>) -> Result<BTreeSet<Path>, Error> {
    let since = since.to_string();

    if let Some(head) = head {
        let changed_files = ScriptEnvironment::new()?
            .with_cwd(project.project_cwd.clone())
            .run_exec("git", ["diff", "--name-only", "--relative", &since, head])
            .await?
            .ok()?
            .stdout_text()?
            .lines()
            .filter(|s| !s.is_empty())
            .map(|s| project.project_cwd.with_join_str(s))
            .collect::<BTreeSet<_>>();

        return Ok(changed_files);
    }

    let local_stdout = ScriptEnvironment::new()?
        .with_cwd(project.project_cwd.clone())
        // --relative makes git print the paths relative to the cwd (and skip
        // the changes located outside of it), which is exactly the project's
        // scope; without it a project in a subdirectory gets bogus paths.
        .run_exec("git", ["diff", "--name-only", "--relative", &since])
        .await?
        .ok()?
        .stdout_text()?
        .lines()
        .map(|s| project.project_cwd.with_join_str(s))
        .collect::<Vec<_>>();

    let untracked_stdout = ScriptEnvironment::new()?
        .with_cwd(project.project_cwd.clone())
        .run_exec("git", ["ls-files", "--others", "--exclude-standard"])
        .await?
        .ok()?
        .stdout_text()?
        .lines()
        .map(|s| project.project_cwd.with_join_str(s))
        .collect::<Vec<_>>();

    let changed_files
        = local_stdout.into_iter()
            .chain(untracked_stdout.into_iter())
            .collect::<BTreeSet<_>>();

    Ok(changed_files)
}

/// Range of changes to consider for change detection.
#[derive(Debug, Clone, Default)]
pub struct ChangesetRange {
    /// Base ref; defaults to the merge base with `changesetBaseRefs`
    pub base: Option<String>,
    /// Head ref; defaults to the working tree (including uncommitted and untracked files)
    pub head: Option<String>,
}

impl ChangesetRange {
    pub fn since(base: Option<&str>) -> Self {
        Self {
            base: base.map(|base| base.to_string()),
            head: None,
        }
    }

    /// Fill the unset bounds from `YARN_CHANGESET_BASE`/`YARN_CHANGESET_HEAD`
    /// (falling back to turbo's `TURBO_SCM_BASE`/`TURBO_SCM_HEAD` to ease
    /// migrations).
    pub fn with_env_defaults(mut self) -> Self {
        let read = |names: [&str; 2]| names.iter()
            .find_map(|name| std::env::var(name).ok().filter(|value| !value.is_empty()));

        if self.base.is_none() {
            self.base = read(["YARN_CHANGESET_BASE", "TURBO_SCM_BASE"]);
        }

        if self.head.is_none() {
            self.head = read(["YARN_CHANGESET_HEAD", "TURBO_SCM_HEAD"]);
        }

        self
    }
}

/// Changed workspaces in the given range. If any changed file matches the
/// `changesetGlobalFiles` setting, every workspace is considered changed
/// (turbo's `globalDependencies`).
pub async fn fetch_changed_workspaces_in_range(project: &Project, range: &ChangesetRange) -> Result<BTreeSet<Ident>, Error> {
    // Without an explicit base, compare against where the head ref (not
    // the current checkout) forked from the base branch
    let since_ref = match &range.base {
        Some(base) => base.clone(),
        None => fetch_branch_base_of(project, range.head.as_deref().unwrap_or("HEAD")).await?,
    };

    let mut changed_files
        = fetch_changed_files_between(project, &since_ref, range.head.as_deref()).await?;

    // Install artifacts don't make the root workspace itself change; the
    // lockfile is compared separately
    changed_files.retain(|file| !is_install_artifact(project, file));

    if touches_global_files(project, &changed_files) {
        return Ok(project.workspaces.iter().map(|w| w.name.clone()).collect());
    }

    let changed_workspaces
        = changed_workspaces_from_files(project, &since_ref, range.head.as_deref(), &changed_files).await?;

    Ok(changed_workspaces.into_keys().collect())
}

fn is_install_artifact(project: &Project, file: &Path) -> bool {
    let Some(rel_path) = file.forward_relative_to(&project.project_cwd) else {
        return false;
    };

    let rel_path
        = rel_path.as_str();

    rel_path == ".pnp.cjs"
        || rel_path == ".pnp.loader.mjs"
        || rel_path == ".yarn"
        || rel_path.starts_with(".yarn/")
}

fn touches_global_files(project: &Project, changed_files: &BTreeSet<Path>) -> bool {
    let patterns: Vec<zpm_utils::Glob>
        = project.config.settings.changeset_global_files.iter()
            .filter_map(|pattern| zpm_utils::Glob::parse(pattern.value.as_str()).ok())
            .collect();

    if patterns.is_empty() {
        return false;
    }

    changed_files.iter().any(|file| {
        let Some(rel_path) = file.forward_relative_to(&project.project_cwd) else {
            return false;
        };

        patterns.iter().any(|pattern| pattern.is_match(rel_path.as_str()))
    })
}

/// Workspaces affected by the changes in the range: the changed workspaces
/// (lockfile-aware, global files included) and, transitively, every
/// workspace depending on them. Equivalent to `turbo ls --affected`.
pub async fn fetch_affected_workspaces(project: &Project, range: &ChangesetRange) -> Result<BTreeSet<Ident>, Error> {
    let changed
        = fetch_changed_workspaces_in_range(project, range).await?;

    Ok(project.workspaces_with_dependents(&changed))
}
