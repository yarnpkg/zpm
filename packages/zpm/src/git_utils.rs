use std::{
    collections::{BTreeMap, BTreeSet},
    io::{BufRead, BufReader, Read, Write},
    process::{Command, Stdio},
    sync::Arc,
};

use itertools::Itertools;
use zpm_parsers::JsonDocument;
use zpm_primitives::Ident;
use zpm_utils::{Path, ToFileString};

use crate::{
    error::Error,
    lockfile::Lockfile,
    manifest::helpers::parse_manifest,
    project::{
        walk_lockfile_workspaces,
        Project,
        Workspace,
        WorkspaceInfo,
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
            // Stored hashes are the fast path; when a side doesn't
            // carry them (enableWorkspaceHashes off, or a lockfile
            // predating them), compute them on demand. Both paths use
            // the same deterministic function, so mixing a stored
            // side with an on-demand side stays valid.
            let current_hashes
                = if current.workspaces.is_empty() {
                    project.workspace_hashes_ondemand(current)
                } else {
                    Some(current.workspaces.clone())
                };

            let old_hashes
                = if old.workspaces.is_empty() {
                    // Hard timeout around the git archaeology: a wedged
                    // fetch must degrade to pre-#256 attribution rather
                    // than hang `foreach --since` forever.
                    tokio::time::timeout(
                        std::time::Duration::from_secs(60),
                        fetch_workspaces_at_ref(project, &since_ref),
                    ).await.ok()
                        .and_then(|result| result.ok())
                        .and_then(|old_workspaces| walk_lockfile_workspaces(
                            old,
                            &old_workspaces,
                            project.config.settings.enable_transparent_workspaces.value,
                            true,
                        ).map(|walk| walk.workspace_hashes))
                } else {
                    Some(old.workspaces.clone())
                };

            if let (Some(current_hashes), Some(old_hashes)) = (current_hashes, old_hashes) {
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
                                .insert(lockfile_path.clone());
                        }
                    }
                }
            }
        }
    }

    Ok(changed_workspaces)
}

/// Fetches the workspace manifests at the given git ref in a single
/// `git cat-file --batch` conversation and rebuilds `Workspace`s
/// from them, so the walk in `fetch_changed_workspaces` can hash the
/// dependency trees as they were at the ref. Each request is written
/// and its response fully read before the next request goes out, so
/// neither pipe can ever overflow. Manifests missing at the ref
/// (untracked, renamed workspaces) or failing to parse are silently
/// skipped, so the caller can degrade gracefully.
async fn fetch_workspaces_at_ref(project: &Project, git_ref: &str) -> Result<Vec<Workspace>, Error> {
    let manifest_paths: Vec<(Path, String)>
        = project.workspaces.iter()
            .map(|workspace| {
                let git_path = if workspace.rel_path == Path::new() {
                    "package.json".to_string()
                } else {
                    format!("{}/package.json", workspace.rel_path.to_file_string())
                };

                (workspace.rel_path.clone(), git_path)
            })
            .collect();

    let rel_paths: Vec<Path>
        = manifest_paths.iter()
            .map(|(rel_path, _)| rel_path.clone())
            .collect();

    let cwd
        = project.project_cwd.clone();

    let git_ref
        = git_ref.to_string();

    let contents
        = tokio::task::spawn_blocking(move || -> Result<Vec<Option<String>>, Error> {
            let spawn_error = |error: std::io::Error| Error::SpawnFailed {
                name: "git".to_string(),
                path: cwd.clone(),
                error: Arc::new(Box::new(error)),
            };

            let mut child
                = Command::new("git")
                    .args(["cat-file", "--batch"])
                    .current_dir(cwd.to_path_buf())
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null())
                    .spawn()
                    .map_err(spawn_error)?;

            let mut child_stdin
                = child.stdin.take().unwrap();

            let mut child_stdout
                = BufReader::new(child.stdout.take().unwrap());

            let mut contents
                = Vec::with_capacity(manifest_paths.len());

            for (_, git_path) in &manifest_paths {
                let request
                    = format!("{}:{}\n", git_ref, git_path);

                child_stdin.write_all(request.as_bytes())
                    .and_then(|_| child_stdin.flush())
                    .map_err(spawn_error)?;

                let mut header
                    = String::new();

                child_stdout.read_line(&mut header)
                    .map_err(spawn_error)?;

                // `<oid> <type> <size>` on success, `<request> missing`
                // when the path doesn't exist at the ref.
                let Some(size) = header.trim_end().rsplit(' ').next().and_then(|field| field.parse::<usize>().ok()) else {
                    contents.push(None);
                    continue;
                };

                let mut content
                    = vec![0u8; size];

                // Every object ends with a newline separator.
                let mut separator
                    = [0u8; 1];

                child_stdout.read_exact(&mut content)
                    .and_then(|_| child_stdout.read_exact(&mut separator))
                    .map_err(spawn_error)?;

                contents.push(Some(String::from_utf8_lossy(&content).to_string()));
            }

            drop(child_stdin);

            if !child.wait().map_err(spawn_error)?.success() {
                return Err(Error::ChildProcessFailed("git".to_string()));
            }

            Ok(contents)
        }).await.map_err(|_| Error::ChildProcessFailed("git".to_string()))??;

    let mut workspaces = Vec::new();

    for (rel_path, manifest_content) in rel_paths.iter().zip(contents) {
        let Some(manifest_content) = manifest_content else {
            continue;
        };

        let Ok(manifest) = parse_manifest(&manifest_content) else {
            continue;
        };

        workspaces.push(Workspace::from_info(&project.project_cwd, WorkspaceInfo {
            rel_path: rel_path.clone(),
            manifest,
            last_changed_at: 0,
        })?);
    }

    Ok(workspaces)
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
