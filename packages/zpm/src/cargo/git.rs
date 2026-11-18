//! Git dependencies.
//!
//! A git dependency names a repository and (optionally) a branch, a tag or
//! a revision. Cargo then looks for the crate anywhere in the repository,
//! by its name. Checkouts are kept in the global folder, one per commit.

use std::{collections::BTreeMap, sync::{Arc, LazyLock}};

use dashmap::DashMap;
use tokio::sync::OnceCell;
use zpm_git::{GitRange, GitSource, GitTreeish, PrepareParams};
use zpm_primitives::{CargoGitRef, CargoGitSource};
use zpm_utils::{Hash64, Path, ToFileString};

use crate::{
    error::Error,
    git::{make_git_env, resolve_git_treeish, validate_repo_url},
    script::ScriptEnvironment,
};

use super::{CargoContext, CrateSource, Summary, manifest::{Manifest, PathDependencies}};

const CHECKOUT_MARKER: &str
    = ".yarn-checkout";

fn is_full_commit(rev: &str) -> bool {
    rev.len() == 40 && rev.bytes().all(|c| c.is_ascii_hexdigit())
}

async fn git(cwd: &Path, args: &[&str]) -> Result<String, Error> {
    let output = ScriptEnvironment::new()?
        .with_cwd(cwd.clone())
        .with_env(make_git_env())
        .run_exec("git", args)
        .await?
        .ok()?
        .output();

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Finds the commit a git dependency currently points to.
pub async fn resolve_commit(context: &CargoContext, source: &CargoGitSource) -> Result<String, Error> {
    let treeish = match source.git_ref() {
        CargoGitRef::DefaultBranch => GitTreeish::Head("HEAD".to_string()),
        CargoGitRef::Branch(branch) => GitTreeish::Head(branch),
        CargoGitRef::Tag(tag) => GitTreeish::Tag(tag),
        CargoGitRef::Rev(rev) if is_full_commit(&rev) => GitTreeish::Commit(rev.to_ascii_lowercase()),

        // Abbreviated revisions can only be expanded with the history at hand
        CargoGitRef::Rev(rev) => {
            validate_repo_url(source.url(), &context.http_client.config, &context.approved_git_repositories)?;

            let clone_dir
                = Path::temp_dir()?;

            git(&clone_dir, &["clone", "--quiet", "--no-checkout", source.url(), "."]).await?;

            return git(&clone_dir, &["rev-parse", &format!("{}^{{commit}}", rev)]).await;
        },
    };

    let range = GitRange {
        repo: GitSource::Url(source.url().to_string()),
        treeish,
        prepare_params: PrepareParams::default(),
    };

    resolve_git_treeish(&range, &context.http_client.config, &context.approved_git_repositories).await
}

static CHECKOUTS: LazyLock<DashMap<String, Arc<OnceCell<Path>>>>
    = LazyLock::new(DashMap::new);

fn checkout_path(context: &CargoContext, source: &CargoGitSource, commit: &str) -> Path {
    context.cache_dir
        .with_join_str("git")
        .with_join_str(format!("{}-{}", Hash64::from_data(source.url().as_bytes()).mini(), commit))
}

/// Returns the folder holding the repository at the given commit, cloning
/// it if it isn't in the global folder yet.
pub async fn ensure_checkout(context: &CargoContext, source: &CargoGitSource, commit: &str) -> Result<Path, Error> {
    let checkout_dir
        = checkout_path(context, source, commit);

    let cell
        = CHECKOUTS.entry(checkout_dir.to_file_string()).or_default().clone();

    let checkout_dir = cell.get_or_try_init(|| async {
        if checkout_dir.with_join_str(CHECKOUT_MARKER).fs_exists() {
            return Ok(checkout_dir);
        }

        validate_repo_url(source.url(), &context.http_client.config, &context.approved_git_repositories)?;

        let clone_dir
            = Path::temp_dir()?;

        git(&clone_dir, &["init", "--quiet"]).await?;

        // Most servers let a single commit be fetched; the others need
        // the whole history
        let shallow
            = git(&clone_dir, &["fetch", "--quiet", "--depth", "1", source.url(), commit]).await;

        let revision = match shallow {
            Ok(_) => "FETCH_HEAD",

            Err(_) => {
                git(&clone_dir, &["fetch", "--quiet", "--tags", source.url(), "+refs/heads/*:refs/remotes/origin/*"]).await?;
                commit
            },
        };

        git(&clone_dir, &["-c", "advice.detachedHead=false", "-c", "core.autocrlf=false", "checkout", "--quiet", revision]).await?;

        // Only the files matter from now on
        clone_dir.with_join_str(".git").fs_rm()?;
        clone_dir.with_join_str(CHECKOUT_MARKER).fs_write_text(commit)?;

        checkout_dir.fs_create_parent()?;

        // Another process may have got there first
        if clone_dir.fs_rename(&checkout_dir).is_err() && !checkout_dir.with_join_str(CHECKOUT_MARKER).fs_exists() {
            clone_dir.fs_move(&checkout_dir)?;
        }

        Ok::<_, Error>(checkout_dir)
    }).await?;

    Ok(checkout_dir.clone())
}

/// A crate found in a checkout.
#[derive(Clone, Debug)]
pub struct GitPackage {
    pub manifest: Manifest,
    pub workspace: Option<Manifest>,
    pub summary: Arc<Summary>,
}

fn collect_manifests(dir: &Path, manifests: &mut Vec<Manifest>) {
    if Manifest::exists(dir) {
        if let Ok(manifest) = Manifest::read(dir) {
            manifests.push(manifest);
        }
    }

    let Ok(entries) = dir.fs_read_dir() else {
        return;
    };

    let mut children = entries
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_ok_and(|file_type| file_type.is_dir()))
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| name != ".git" && name != "target")
        .collect::<Vec<_>>();

    children.sort();

    for child in children {
        collect_manifests(&dir.with_join_str(child), manifests);
    }
}

/// Lists the crates of a checkout, by name. When a repository holds
/// several crates with the same name, the one closest to the root wins.
pub fn read_packages(checkout_dir: &Path, source: &CargoGitSource, commit: &str) -> Result<BTreeMap<String, GitPackage>, Error> {
    let mut manifests
        = Vec::new();

    collect_manifests(checkout_dir, &mut manifests);

    manifests.sort_by_key(|manifest| manifest.dir.as_str().matches('/').count());

    let mut packages
        = BTreeMap::new();

    for manifest in manifests {
        let Some(name) = manifest.package_name().map(|name| name.to_string()) else {
            continue;
        };

        if packages.contains_key(&name) {
            continue;
        }

        let workspace
            = manifest.find_workspace_root(Some(checkout_dir))?;

        let crate_source = CrateSource::Git {
            source: source.clone(),
            commit: commit.to_string(),
        };

        // Crates that can't be read (examples, test fixtures...) only
        // matter if something depends on them, which then fails by itself
        let Ok(summary) = manifest.summarize(workspace.as_ref(), crate_source, &PathDependencies::Git(source)) else {
            continue;
        };

        packages.insert(name, GitPackage {
            manifest,
            workspace,
            summary: Arc::new(summary),
        });
    }

    Ok(packages)
}
