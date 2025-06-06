use std::collections::BTreeSet;

use zpm_utils::Path;

use crate::{error::Error, script::ScriptEnvironment};

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
        .await
        .ok()?
        .stdout_text()?;

    Ok(title)
}

pub async fn fetch_base(root: &Path, base_refs: &[&str]) -> Result<String, Error> {
    let mut ancestor_bases
        = Vec::new();

    for &candidate in base_refs {
        let code = ScriptEnvironment::new()?
            .with_cwd(root.clone())
            .run_exec("git", ["merge-base", candidate, "HEAD"])
            .await;

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
        .await
        .ok()?
        .stdout_text()?;

    Ok(merge_base)
}

pub async fn fetch_changed_files(root: &Path, base: &str) -> Result<BTreeSet<Path>, Error> {
    let local_stdout = ScriptEnvironment::new()?
        .with_cwd(root.clone())
        .run_exec("git", ["diff", "--name-only", base])
        .await
        .ok()?
        .stdout_text()?
        .lines()
        .map(|s| root.with_join_str(s))
        .collect::<Vec<_>>();

    let untracked_stdout = ScriptEnvironment::new()?
        .with_cwd(root.clone())
        .run_exec("git", ["ls-files", "--others", "--exclude-standard"])
        .await
        .ok()?
        .stdout_text()?
        .lines()
        .map(|s| root.with_join_str(s))
        .collect::<Vec<_>>();

    let changed_files
        = local_stdout.into_iter()
            .chain(untracked_stdout.into_iter())
            .collect::<BTreeSet<_>>();

    Ok(changed_files)
}
