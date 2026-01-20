use std::{collections::BTreeSet, fs::{FileType, Metadata}};

use zpm_utils::Path;

use crate::{diff_finder::{CacheState, DiffController, DiffFinder}, error::Error, manifest::{helpers::read_manifest_with_size, Manifest}};

#[derive(Debug)]
pub enum PollResult {
    Changed,
    NotFound,
}

pub trait ManifestFinder {
    fn rsync(&mut self) -> Result<Vec<Path>, Error>;
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
pub struct CachedManifestFinder {
    pub diff_finder: DiffFinder<CachedManifestFinder>,
    pub save_state_path: Path,
}

impl CachedManifestFinder {
    pub fn new(root_path: Path) -> Self {
        let save_state_path
            = root_path.with_join_str(".yarn/ignore/manifests");

        let save_state
            = save_state_path
                .fs_read_prealloc()
                .ok()
                .and_then(|save_data| CacheState::from_slice(&save_data).ok())
                .unwrap_or_default();

        let roots
            = vec![Path::new()];

        Self {
            diff_finder: DiffFinder::new(root_path, roots, save_state),
            save_state_path,
        }
    }

    fn save(&self) -> Result<(), Error> {
        let data
            = self.diff_finder.state.to_vec()?;

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

    pub fn into_state(self) -> CacheState<Manifest> {
        self.diff_finder.into_state()
    }
}

impl DiffController for CachedManifestFinder {
    type Data = Manifest;

    fn is_relevant_entry(file_name: &str, file_type: &FileType) -> bool {
        if file_type.is_dir() {
            return file_name != ".yarn" && file_name != "node_modules";
        }

        if file_type.is_file() {
            return file_name == "package.json";
        }

        false
    }

    fn get_file_data(path: &Path, metadata: &Metadata) -> Result<Self::Data, Error> {
        read_manifest_with_size(&path, metadata.len())
    }
}
