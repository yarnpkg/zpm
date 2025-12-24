use std::{collections::BTreeSet, fs::{DirEntry, FileType, Metadata}, sync::Arc};

use ignore::gitignore::Gitignore;
use zpm_utils::Path;

use crate::{diff_finder::{DiffController, DiffFinder, NoFilter, PathFilter, SaveState}, error::Error, manifest::{helpers::read_manifest_with_size, Manifest}};

#[derive(Debug)]
pub enum PollResult {
    Changed,
    NotFound,
}

pub trait ManifestFinder {
    fn rsync(&mut self) -> Result<Vec<Path>, Error>;
}

/// A path filter that uses gitignore patterns to exclude directories.
#[derive(Debug)]
pub struct GitignoreFilter {
    gitignore: Gitignore,
}

impl GitignoreFilter {
    /// Creates a new GitignoreFilter from the project root path.
    /// Reads and parses the .gitignore file if it exists.
    pub fn from_project_root(project_root: &Path) -> Option<Self> {
        let gitignore_path = project_root.with_join_str(".gitignore");

        if !gitignore_path.fs_exists() {
            return None;
        }

        let mut builder = ignore::gitignore::GitignoreBuilder::new(project_root.as_str());
        if builder.add(gitignore_path.as_str()).is_some() {
            // Error reading gitignore, skip
            return None;
        }

        builder.build().ok().map(|gitignore| Self { gitignore })
    }
}

impl PathFilter for GitignoreFilter {
    fn is_excluded(&self, rel_path: &Path) -> bool {
        // The gitignore crate expects paths relative to the repo root
        // For directories, we need to add a trailing slash
        let path_str = format!("{}/", rel_path.as_str());
        self.gitignore.matched_path_or_any_parents(&path_str, true).is_ignore()
    }
}

/**
 * The CachedManifestFinder struct is meant to very quickly locate all the
 * manifests in a given directory, no matter how deep the directory structure
 * is, by caching the mtime of each directory it checks.
 *
 * This strategy is similar to how `git status` works; subsequent invocations
 * only need to compare the cached mtime for each directory with the current
 * mtime to figure out whether they need perform the costly readdir syscall.
 */
#[derive(Debug)]
pub struct CachedManifestFinder<TFilter: PathFilter = NoFilter> {
    pub diff_finder: DiffFinder<CachedManifestFinderController, TFilter>,
    pub save_state_path: Path,
}

/// The DiffController implementation for manifest finding.
/// Separated from CachedManifestFinder to avoid self-referential generic issues.
#[derive(Debug, Default)]
pub struct CachedManifestFinderController;

impl Default for CachedManifestFinder<NoFilter> {
    fn default() -> Self {
        Self {
            diff_finder: DiffFinder::default(),
            save_state_path: Path::new(),
        }
    }
}

impl CachedManifestFinder<NoFilter> {
    pub fn new(root_path: Path) -> Self {
        let save_state_path = root_path
            .with_join_str(".yarn/ignore/manifests");

        // We tolerate any errors; worst case, we'll just re-scan the entire
        // directory to rebuild the cache.
        let save_state = save_state_path
            .fs_read_prealloc()
            .ok()
            .and_then(|save_data| SaveState::from_slice(&save_data).ok())
            .unwrap_or_default();

        Self {
            diff_finder: DiffFinder::new(root_path, save_state),
            save_state_path,
        }
    }
}

impl CachedManifestFinder<GitignoreFilter> {
    /// Creates a new CachedManifestFinder that respects .gitignore patterns.
    pub fn new_with_gitignore(root_path: Path, gitignore_filter: GitignoreFilter) -> Self {
        let save_state_path = root_path
            .with_join_str(".yarn/ignore/manifests");

        // We tolerate any errors; worst case, we'll just re-scan the entire
        // directory to rebuild the cache.
        let save_state = save_state_path
            .fs_read_prealloc()
            .ok()
            .and_then(|save_data| SaveState::from_slice(&save_data).ok())
            .unwrap_or_default();

        Self {
            diff_finder: DiffFinder::new_with_filter(root_path, save_state, Arc::new(gitignore_filter)),
            save_state_path,
        }
    }
}

impl<TFilter: PathFilter> CachedManifestFinder<TFilter> {
    fn save(&self) -> Result<(), Error> {
        let data
            = self.diff_finder.save_state.to_vec()?;

        // We don't care about write errors, as it may be due to read-only
        // filesystems which were modified after we first scanned the filesystem
        // (e.g. Docker images with COPY call right between a Yarn command and
        // a USER directive); in the worst case Yarn commands will just need
        // to re-scan some directories, but that's not that big a deal, especially
        // within containers.
        let _
            = self.save_state_file(&data);

        Ok(())
    }

    fn save_state_file(&self, data: &[u8]) -> Result<(), Error> {
        self.save_state_path
            .fs_create_parent()?
            .fs_write(&data)?;

        Ok(())
    }

    pub fn rsync(&mut self) -> Result<(bool, BTreeSet<Path>), Error> {
        let (has_changed, changeset)
            = self.diff_finder.rsync()?;

        if has_changed {
            self.save()?;
        }

        Ok((has_changed, changeset))
    }

    pub fn into_state(self) -> SaveState<Manifest> {
        self.diff_finder.save_state
    }
}

impl DiffController for CachedManifestFinderController {
    type Data = Manifest;

    fn is_relevant_entry(path: &DirEntry, file_type: &FileType) -> bool {
        if file_type.is_dir() {
            return path.file_name() != ".yarn" && path.file_name() != "node_modules";
        }

        if file_type.is_file() {
            return path.file_name() == "package.json";
        }

        false
    }

    fn get_file_data(path: &Path, metadata: &Metadata) -> Result<Self::Data, Error> {
        read_manifest_with_size(&path, metadata.len())
    }
}
