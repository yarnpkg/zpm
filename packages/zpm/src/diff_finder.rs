use std::{collections::{BTreeMap, BTreeSet}, fs::{DirEntry, FileType, Metadata}, fmt::Debug, io::ErrorKind, time::UNIX_EPOCH};

use bincode::{Decode, Encode};
use rayon::iter::{IntoParallelIterator, IntoParallelRefIterator, ParallelIterator};
use zpm_utils::Path;

use crate::error::Error;

#[derive(Debug)]
enum CacheCheck<T> {
    Skip,
    NotFound(Path),
    StableFile(Path),
    ChangedFile(Path, u128, T),
    ChangedDirectory(Path, u128),
}

/**
 * A save entry contains the mtime since the last modification, and arbitrary
 * data provided by the DiffController trait.
 */
#[derive(Debug, Encode, Decode)]
pub enum SaveEntry<T> {
    File(u128, T),
    Directory(u128),
}

impl<T> SaveEntry<T> {
    pub fn mtime(&self) -> u128 {
        match self {
            SaveEntry::File(mtime, _) => *mtime,
            SaveEntry::Directory(mtime) => *mtime,
        }
    }
}

/**
 * The save state contains the list of relevant files found on disk. It can be
 * persisted in a file and
 */
#[derive(Default, Debug, Encode, Decode)]
pub struct SaveState<T> {
    pub cache: BTreeMap<Path, SaveEntry<T>>,
    pub roots: Vec<Path>,
}

impl<T> SaveState<T> {
    pub fn new(roots: Vec<Path>) -> Self {
        Self {
            cache: BTreeMap::from_iter(roots.iter().map(|root| (root.clone(), SaveEntry::Directory(0)))),
            roots,
        }
    }

    pub fn from_slice(data: &[u8]) -> Result<Self, Error> where T: Decode<()> {
        let (save_state, _)
            = bincode::decode_from_slice::<SaveState<T>, _>(data, bincode::config::standard())
                .map_err(|_| Error::ReplaceMe)?;

        Ok(save_state)
    }

    pub fn to_vec(&self) -> Result<Vec<u8>, Error> where T: Encode {
        Ok(bincode::encode_to_vec(self, bincode::config::standard())?)
    }
}

/**
 * This trait lets the diff owner filter the list of files we care about, and
 * tweak what we store inside the save state.
 */
pub trait DiffController {
    type Data: Debug + Send + Sync;

    fn is_relevant_entry(entry: &DirEntry, file_type: &FileType) -> bool;
    fn get_file_data(path: &Path, metadata: &Metadata) -> Result<Self::Data, Error>;
}

/**
 * The DiffFinder struct is meant to very quickly locate everything that changed
 * in a given directory between two rsync calls. The returned "save state" can
 * be serialized on disk, allowing this implementation to track changes even
 * across different CLI calls.
 *
 * This strategy is similar to how `git status` works; subsequent invocations
 * only need to compare the cached mtime for each directory with the current
 * mtime to figure out whether they need perform the costly readdir syscall.
 */
#[derive(Default, Debug)]
pub struct DiffFinder<TController: DiffController> {
    pub root_path: Path,
    pub save_state: SaveState<TController::Data>,
}

impl<TController: DiffController> DiffFinder<TController> {
    pub fn new(root_path: Path, mut save_state: SaveState<TController::Data>) -> Self {
        let roots = vec![
            Path::new(),
        ];

        if save_state.roots != roots {
            save_state = SaveState::new(roots);
        }

        Self {
            root_path,
            save_state,
        }
    }

    fn refresh_directory(&mut self, new_file_paths: &mut Vec<Path>, rel_path: &Path, current_time: u128) -> Result<(), Error> {
        self.save_state.cache.insert(
            rel_path.clone(),
            SaveEntry::Directory(current_time),
        );

        let abs_path = self.root_path
            .with_join(rel_path);

        let directory_entries = abs_path.fs_read_dir()?
            .into_iter()
            .collect::<Result<Vec<_>, _>>()?;

        for entry in directory_entries {
            let file_type
                = entry.file_type()?;

            if !TController::is_relevant_entry(&entry, &file_type) {
                continue;
            }

            let entry_rel_path = rel_path
                .with_join_str(entry.file_name().to_str().unwrap());

            if self.save_state.cache.contains_key(&entry_rel_path) {
                continue;
            }

            if file_type.is_dir() {
                self.refresh_directory(new_file_paths, &entry_rel_path, current_time)?;
            } else {
                new_file_paths.push(entry_rel_path.clone());
            }
        }

        Ok(())
    }

    pub fn rsync(&mut self) -> Result<(bool, BTreeSet<Path>), Error> {
        let cache_checks = self.save_state.cache.par_iter()
            .map(|(rel_path, save_entry)| {
                let abs_path = self.root_path
                    .with_join(&rel_path);

                let metadata = match abs_path.fs_metadata() {
                    Ok(metadata) => {
                        metadata
                    },

                    Err(e) if e.io_kind() == Some(ErrorKind::NotFound) => {
                        return Ok(CacheCheck::NotFound(rel_path.clone()))
                    },

                    Err(e) => {
                        return Err(e.into())
                    },
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
                        Ok(CacheCheck::ChangedFile(rel_path.clone(), mtime, TController::get_file_data(&abs_path, &metadata)?))
                    } else {
                        Ok(CacheCheck::StableFile(rel_path.clone()))
                    }
                }
            })
            .collect::<Result<Vec<_>, Error>>()?;

        let mut has_changed = false;
        let mut new_file_paths = vec![];
        let mut all_changed_paths = BTreeSet::new();

        for cache_check in cache_checks {
            match cache_check {
                CacheCheck::Skip => {
                    // Nothing to do, it's just a directory that didn't change
                },

                CacheCheck::StableFile(_) => {
                    // Nothing to do, it's already in the cache
                },

                CacheCheck::ChangedFile(rel_path, mtime, data) => {
                    self.save_state.cache.insert(rel_path.clone(), SaveEntry::File(mtime, data));
                    all_changed_paths.insert(rel_path);
                    has_changed = true;
                },

                CacheCheck::ChangedDirectory(rel_path, mtime) => {
                    self.refresh_directory(&mut new_file_paths, &rel_path, mtime)?;
                    has_changed = true;
                },

                CacheCheck::NotFound(rel_path) => {
                    self.save_state.cache.remove(&rel_path);
                    has_changed = true;
                },
            }
        }

        let new_entries = new_file_paths.into_par_iter()
            .map(|rel_path| {
                let abs_path = self.root_path
                    .with_join(&rel_path);

                let metadata
                    = abs_path.fs_metadata()?;
                let data
                    = TController::get_file_data(&abs_path, &metadata)?;

                let mtime = metadata.modified()?
                    .duration_since(UNIX_EPOCH)?
                    .as_nanos() as u128;

                Ok((rel_path, SaveEntry::File(mtime, data)))
            })
            .collect::<Result<Vec<_>, Error>>()?;

        for (rel_path, save_entry) in new_entries {
            self.save_state.cache.insert(rel_path.clone(), save_entry);
            all_changed_paths.insert(rel_path);
        }

        Ok((has_changed, all_changed_paths))
    }
}
