use std::{collections::BTreeMap, io, time::UNIX_EPOCH};

use bincode::{Decode, Encode};
use rayon::iter::{IntoParallelIterator, IntoParallelRefIterator, ParallelIterator};
use zpm_utils::Path;

use crate::{error::Error, manifest::{helpers::read_manifest_with_size, Manifest}};

#[derive(Debug, Encode, Decode)]
pub enum SaveEntry {
    Manifest(u128, Manifest),
    Directory(u128),
}

impl SaveEntry {
    pub fn mtime(&self) -> u128 {
        match self {
            SaveEntry::Manifest(mtime, _) => *mtime,
            SaveEntry::Directory(mtime) => *mtime,
        }
    }
}

#[derive(Default, Debug, Encode, Decode)]
pub struct SaveState {
    pub cache: BTreeMap<Path, SaveEntry>,
    pub roots: Vec<Path>,
}

impl SaveState {
    pub fn new(roots: Vec<Path>) -> Self {
        Self {
            cache: BTreeMap::from_iter(roots.iter().map(|root| (root.clone(), SaveEntry::Directory(0)))),
            roots,
        }
    }
}

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
#[derive(Default, Debug)]
pub struct CachedManifestFinder {
    pub root_path: Path,
    pub save_state_path: Path,
    pub save_state: SaveState,
}

impl CachedManifestFinder {
    pub fn new(root_path: Path) -> Result<Self, Error> {
        let save_state_path = root_path
            .with_join_str(".yarn/ignore/manifests");

        let roots = vec![
            Path::new(),
        ];

        // We tolerate any errors; worst case, we'll just re-scan the entire
        // directory to rebuild the cache.
        let mut save_state = save_state_path
            .fs_read_prealloc()
            .ok()
            .and_then(|save_data| bincode::decode_from_slice::<SaveState, _>(save_data.as_slice(), bincode::config::standard()).ok())
            .map(|(save_state, _)| save_state)
            .unwrap_or_default();

        if save_state.roots != roots {
            println!("Save state roots don't match; rebuilding cache");
            save_state = SaveState::new(roots);
        }

        Ok(Self {
            root_path,
            save_state_path,
            save_state,
        })
    }

    fn save(&self) -> Result<(), Error> {
        let data = bincode::encode_to_vec(
            &self.save_state,
            bincode::config::standard(),
        )?;

        self.save_state_path
            .fs_create_parent()?
            .fs_write(&data)?;

        Ok(())
    }

    fn refresh_directory(&mut self, manifest_paths: &mut Vec<Path>, rel_path: &Path, current_time: u128) -> Result<(), Error> {
        self.save_state.cache.insert(rel_path.clone(), SaveEntry::Directory(current_time));

        let abs_path = self.root_path
            .with_join(rel_path);

        let directory_entries = abs_path.fs_read_dir()?
            .into_iter()
            .collect::<Result<Vec<_>, _>>()?;

        for entry in directory_entries {
            let is_dir = entry.file_type()?.is_dir();

            if !is_dir && entry.file_name() != "package.json" {
                continue;
            }

            if is_dir && entry.file_name() == ".yarn" {
                continue;
            }

            let entry_rel_path = rel_path
                .with_join_str(entry.file_name().to_str().unwrap());

            if self.save_state.cache.contains_key(&entry_rel_path) {
                continue;
            }

            if is_dir {
                self.refresh_directory(manifest_paths, &entry_rel_path, current_time)?;
            } else {
                manifest_paths.push(entry_rel_path.clone());
            }
        }

        Ok(())
    }
}

impl ManifestFinder for CachedManifestFinder {
    fn rsync(&mut self) -> Result<Vec<Path>, Error> {
        enum CacheCheck {
            Skip,
            NotFound(Path),
            StableFile(Path),
            ChangedFile(Path, u128, Manifest),
            ChangedDirectory(Path, u128),
        }

        let cache_checks = self.save_state.cache.par_iter().map(|(rel_path, save_entry)| {
            let abs_path = self.root_path
                .with_join(&rel_path);

            let metadata = match abs_path.fs_metadata() {
                Ok(metadata) => metadata,
                Err(e) if e.io_kind() == Some(io::ErrorKind::NotFound) => return Ok(CacheCheck::NotFound(rel_path.clone())),
                Err(e) => return Err(e.into()),
            };

            let mtime = metadata.modified()?
                .duration_since(UNIX_EPOCH)?
                .as_nanos() as u128;

            if metadata.is_dir() {
                if mtime > save_entry.mtime() {
                    Ok(CacheCheck::ChangedDirectory(rel_path.clone(), mtime))
                } else {
                    Ok(CacheCheck::Skip)
                }
            } else {
                if mtime > save_entry.mtime() {
                    Ok(CacheCheck::ChangedFile(rel_path.clone(), mtime, read_manifest_with_size(&abs_path, metadata.len())?))
                } else {
                    Ok(CacheCheck::StableFile(rel_path.clone()))
                }
            }
        }).collect::<Result<Vec<_>, Error>>()?;

        let mut has_changed = false;

        let mut manifest_paths = vec![];
        let mut new_manifest_paths = vec![];

        for cache_check in cache_checks {
            match cache_check {
                CacheCheck::Skip => {
                    // Nothing to do, it's just a directory that didn't change
                },

                CacheCheck::StableFile(rel_path) => {
                    manifest_paths.push(rel_path);
                },

                CacheCheck::ChangedFile(rel_path, mtime, manifest) => {
                    self.save_state.cache.insert(rel_path.clone(), SaveEntry::Manifest(mtime, manifest));
                    manifest_paths.push(rel_path);
                    has_changed = true;
                },

                CacheCheck::ChangedDirectory(rel_path, mtime) => {
                    self.refresh_directory(&mut new_manifest_paths, &rel_path, mtime)?;
                    has_changed = true;
                },

                CacheCheck::NotFound(rel_path) => {
                    self.save_state.cache.remove(&rel_path);
                    has_changed = true;
                },
            }
        }

        let new_manifests = new_manifest_paths.into_par_iter()
            .map(|rel_path| {
                let abs_path = self.root_path
                    .with_join(&rel_path);

                let metadata = abs_path.fs_metadata()?;
                let manifest = read_manifest_with_size(&abs_path, metadata.len())?;

                let mtime = metadata.modified()?
                    .duration_since(UNIX_EPOCH)?
                    .as_nanos() as u128;

                Ok((rel_path, SaveEntry::Manifest(mtime, manifest)))
            })
            .collect::<Result<Vec<_>, Error>>()?;

        for (rel_path, save_entry) in new_manifests {
            self.save_state.cache.insert(rel_path.clone(), save_entry);
            manifest_paths.push(rel_path);
        }

        if has_changed {
            self.save()?;
        }

        manifest_paths.sort();
        Ok(manifest_paths)
    }
}
