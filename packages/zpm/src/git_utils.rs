use std::collections::{BTreeMap, BTreeSet};

use itertools::Itertools;
use zpm_parsers::JsonDocument;
use zpm_primitives::Ident;
use zpm_utils::Path;

use crate::{error::Error, lockfile::Lockfile, project::{Project, LOCKFILE_NAME}, script::ScriptEnvironment};

pub fn find_root(initial_cwd: &Path) -> Result<Path, Error> {
    // Note: We can't just use `git rev-parse --show-toplevel`, because on Windows
    // it may return long paths even when the cwd uses short paths.

    for parent in initial_cwd.iter_path().rev() {
        let git_path = parent
            .with_join_str(".git");

        if git_path.fs_exists_blocking() {
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
            = vec!["merge-base".to_string(), "HEAD".to_string()];

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
        = fetch_changed_files(&project, Some(&since_ref)).await?;

    let mut changed_workspaces: BTreeMap<_, BTreeSet<_>>
        = BTreeMap::new();

    let lockfile_path
        = project.project_cwd.with_join_str(LOCKFILE_NAME);

    let lockfile_changed
        = changed_files.contains(&lockfile_path);

    for file in &changed_files {
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

    // If the lockfile changed, compare workspace hashes to find affected workspaces
    if lockfile_changed {
        let current_lockfile
            = project.lockfile().ok();

        let old_lockfile
            = fetch_lockfile_at_ref(project, &since_ref).await.ok();

        if let (Some(current), Some(old)) = (&current_lockfile, &old_lockfile) {
            for workspace in &project.workspaces {
                if changed_workspaces.contains_key(&workspace.name) {
                    continue;
                }

                let current_hash
                    = current.workspaces.get(&workspace.name);
                let old_hash
                    = old.workspaces.get(&workspace.name);

                if current_hash != old_hash {
                    changed_workspaces.entry(workspace.name.clone())
                        .or_default()
                        .insert(lockfile_path.clone());
                }
            }
        }
    }

    Ok(changed_workspaces)
}

/// Fetches and parses the lockfile at a specific git ref.
async fn fetch_lockfile_at_ref(project: &Project, git_ref: &str) -> Result<Lockfile, Error> {
    let lockfile_content
        = ScriptEnvironment::new()?
            .with_cwd(project.project_cwd.clone())
            .run_exec("git", ["show", &format!("{}:{}", git_ref, LOCKFILE_NAME)])
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

    let local_stdout = ScriptEnvironment::new()?
        .with_cwd(project.project_cwd.clone())
        .run_exec("git", ["diff", "--name-only", &since])
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
