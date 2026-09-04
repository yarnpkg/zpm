use std::{
    collections::{BTreeMap, BTreeSet},
};

use itertools::Itertools;
use tokio::process::Command;
use zpm_parsers::JsonDocument;
use zpm_primitives::Ident;
use zpm_utils::{Hash64, IoResultExt, LastModifiedAt, Path, ToFileString};

use crate::{
    error::Error,
    lockfile::Lockfile,
    project::{
        Project,
        Workspace,
        LOCKFILE_NAME,
    },
    script::ScriptEnvironment,
};

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

    let since_ref = ScriptEnvironment::new()?
        .with_cwd(project.project_cwd.clone())
        .run_exec("git", ["rev-parse", "--verify", "--end-of-options", &format!("{}^{{commit}}", since_ref)])
        .await?
        .ok()?
        .stdout_text()?;

    let changed_files
        = fetch_changed_files(&project, Some(&since_ref)).await?;

    let mut changed_workspaces: BTreeMap<_, BTreeSet<_>>
        = BTreeMap::new();

    let lockfile_path
        = project.project_cwd.with_join_str(LOCKFILE_NAME);


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

    // Patch/local content and workspace-only edges can change a tree without
    // changing yarn.lock when its workspace hashes are omitted.
    if !changed_files.is_empty() {
        let current_lockfile
            = project.lockfile().ok();

        let old_lockfile
            = fetch_lockfile_at_ref(project, &since_ref).await.ok();

        if let (Some(current), Some(old)) = (&current_lockfile, &old_lockfile) {
            // Stored hashes are the fast path; when a side doesn't
            // carry them (enableWorkspaceHashes off, or a lockfile
            // predating them), compute them on demand. Both paths use
            // the same deterministic function, so mixing a stored
            // side with an on-demand side stays valid.
            let current_hashes
                = if current.workspaces.is_empty() {
                    project.workspace_hashes_ondemand(current).await?
                } else {
                    current.workspaces.clone()
                };

            let old_hashes
                = if old.workspaces.is_empty() {
                    Some(fetch_workspace_hashes_at_ref(project, &since_ref, old).await?)
                } else {
                    Some(old.workspaces.clone())
                };

            if let Some(old_hashes) = old_hashes {
                for workspace in &project.workspaces {
                    if changed_workspaces.contains_key(&workspace.name) {
                        continue;
                    }

                    // Only a difference between two present hashes marks
                    // a workspace as changed; a hash missing on one side
                    // (setting toggled between refs, untracked or renamed
                    // old workspace) must never flag one by itself.
                    if let (Some(current_hash), Some(old_hash)) = (
                        current_hashes.get(&workspace.name),
                        old_hashes.get(&workspace.name),
                    ) {
                        if current_hash != old_hash {
                            changed_workspaces.entry(workspace.name.clone())
                                .or_default()
                                .insert(changed_files.get(&lockfile_path)
                                    .unwrap_or_else(|| changed_files.first().unwrap()).clone());
                        }
                    }
                }
            }
        }
    }

    Ok(changed_workspaces)
}

/// Owns a private checkout and index; neither the user's index nor worktree is
/// modified. Keeping the complete tree also covers patch and file: inputs without
/// maintaining a second set of dependency-protocol rules here.
struct GitSnapshot(Path);

impl Drop for GitSnapshot {
    fn drop(&mut self) {
        let _ = self.0.fs_rm();
    }
}

async fn fetch_workspace_hashes_at_ref(project: &Project, git_ref: &str, lockfile: &Lockfile) -> Result<BTreeMap<Ident, Hash64>, Error> {
    let git_root
        = find_root(&project.project_cwd)?;
    let snapshot
        = GitSnapshot(Path::temp_dir_pattern("yarn-hashes-<>")?);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        snapshot.0.fs_set_permissions(std::fs::Permissions::from_mode(0o700))?;
    }

    let checkout
        = snapshot.0.with_join_str("tree");
    let index
        = snapshot.0.with_join_str("index");
    checkout.fs_create_dir_all()?;
    // Workspace discovery canonicalizes paths; use the same spelling for the
    // source boundary (notably /var versus /private/var on macOS).
    let checkout
        = checkout.fs_canonicalize()?;

    let prefix
        = format!("--prefix={}/", checkout.to_file_string());

    for args in [
        vec!["-c", "core.sparseCheckout=false", "read-tree", git_ref],
        vec!["checkout-index", "--all", &prefix],
    ] {
        let output = tokio::time::timeout(std::time::Duration::from_secs(120), Command::new("git")
            .args(args)
            .current_dir(git_root.to_path_buf())
            .env("GIT_INDEX_FILE", index.to_path_buf())
            .env("GIT_WORK_TREE", checkout.to_path_buf())
            .kill_on_drop(true)
            .output())
            .await.map_err(|_| Error::TaskTimeout)??;

        if !output.status.success() {
            return Err(Error::ChildProcessFailed("git".to_string()));
        }
    }

    let project_cwd
        = checkout.with_join(&project.project_cwd.relative_to(&git_root));
    let rc_path = project.config.project_config_path.as_ref()
        .and_then(|path| path.forward_relative_to(&project.project_cwd))
        .unwrap_or_else(|| Path::try_from(".yarnrc.yml").unwrap());
    let rc_content = project_cwd.with_join(&rc_path)
        .fs_read_text().ok_missing()?.unwrap_or_else(|| "{}".to_string());
    let config
        = project.config.with_historical_graph_settings(&rc_content).ok_or(Error::Unsupported)?;
    let root
        = Workspace::from_root_path(&project_cwd)?;
    let mut workspaces
        = root.workspaces().await?;
    workspaces.insert(0, root);

    let historical_project = Project {
        workspaces_by_ident: workspaces.iter().enumerate()
            .map(|(idx, workspace)| (workspace.name.clone(), idx)).collect(),
        workspaces_by_rel_path: workspaces.iter().enumerate()
            .map(|(idx, workspace)| (workspace.rel_path.clone(), idx)).collect(),
        workspaces,
        config,
        project_cwd,
        package_cwd: project.package_cwd.clone(),
        shell_cwd: project.shell_cwd.clone(),
        last_modified_at: LastModifiedAt::new(),
        install_state: None,
        http_client: project.http_client.clone(),
        clone_limiter: project.clone_limiter.clone(),
    };

    crate::install::workspace_hashes_from_lockfile(&historical_project, lockfile, Some((&git_root, &checkout))).await
}

/// Fetches and parses the lockfile at a specific git ref.
async fn fetch_lockfile_at_ref(project: &Project, git_ref: &str) -> Result<Lockfile, Error> {
    let git_root
        = find_root(&project.project_cwd)?;
    let lockfile_path
        = project.project_cwd.relative_to(&git_root).with_join_str(LOCKFILE_NAME);
    let lockfile_content
        = ScriptEnvironment::new()?
            .with_cwd(project.project_cwd.clone())
            .run_exec("git", ["show", &format!("{}:{}", git_ref, lockfile_path.to_file_string())])
            .await?
            .ok()?
            .stdout_text()?;

    // No native historical graph is available for empty or legacy Berry files.
    if lockfile_content.is_empty() || lockfile_content.starts_with('#') {
        return Err(Error::Unsupported);
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
