use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Debug,
    fs::{FileType, Metadata},
    io::ErrorKind,
    time::UNIX_EPOCH,
};

use rayon::iter::{IntoParallelIterator, IntoParallelRefIterator, ParallelIterator};
use rkyv::{
    rancor::{Fallible, Source},
    Archive,
};
use zpm_utils::Path;

use crate::error::Error;

#[derive(Debug)]
enum CacheCheck<T> {
    Skip,
    NotFound(Path),
    ChangedFile(Path, u128, Result<T, Error>),
    ChangedDirectory(Path, u128),
}

#[derive(Debug, Archive, rkyv::Serialize, rkyv::Deserialize)]
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

#[derive(Debug)]
pub enum CacheEntry<T> {
    File(u128, T),
    Directory(u128),
}

impl<T> CacheEntry<T> {
    pub fn mtime(&self) -> u128 {
        match self {
            CacheEntry::File(mtime, _) => *mtime,
            CacheEntry::Directory(mtime) => *mtime,
        }
    }
}

#[derive(Debug)]
pub struct FailedFile {
    pub mtime: u128,
    pub error: Error,
}

#[derive(Default, Debug, Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct SaveState<T> {
    pub cache: BTreeMap<Path, SaveEntry<T>>,
    pub roots: Vec<Path>,
    pub retry_files: BTreeSet<Path>,
}

impl<T> SaveState<T> {
    pub fn new(roots: Vec<Path>) -> Self {
        Self {
            cache: BTreeMap::from_iter(
                roots.iter().map(|root| (root.clone(), SaveEntry::Directory(0))),
            ),
            roots,
            retry_files: BTreeSet::new(),
        }
    }

    pub fn from_slice(data: &[u8]) -> Result<Self, Error>
    where
        Self: for<'a> rkyv::Deserialize<Self, rkyv::de::Pool>,
        for<'a> <Self as Archive>::Archived: rkyv::bytecheck::CheckBytes<rkyv::api::high::HighValidator<'a, rkyv::rancor::BoxedError>>,
    {
        rkyv::from_bytes::<Self, rkyv::rancor::BoxedError>(data)
            .map_err(|_| Error::ReplaceMe)
    }

    pub fn to_vec(&self) -> Result<Vec<u8>, Error>
    where
        Self: for<'a> rkyv::Serialize<rkyv::ser::Serializer<rkyv::util::AlignedVec, rkyv::ser::allocator::ArenaHandle<'a>, rkyv::ser::sharing::Share>>,
    {
        rkyv::to_bytes::<rkyv::rancor::BoxedError>(self)
            .map(|v| v.to_vec())
            .map_err(|_| Error::ReplaceMe)
    }
}

pub trait DiffController {
    type Data: Debug + Send + Sync;

    fn is_relevant_entry(file_name: &str, file_type: &FileType) -> bool;
    fn get_file_data(path: &Path, metadata: &Metadata) -> Result<Self::Data, Error>;
}

#[derive(Debug)]
pub struct CacheState<T> {
    pub cache: BTreeMap<Path, CacheEntry<T>>,
    pub roots: Vec<Path>,
    pub failed_files: BTreeMap<Path, FailedFile>,
    retry_files: BTreeSet<Path>,
}

impl<T> CacheState<T> {
    pub fn new(roots: Vec<Path>) -> Self {
        Self {
            cache: BTreeMap::from_iter(
                roots.iter().map(|root| (root.clone(), CacheEntry::Directory(0))),
            ),
            roots,
            failed_files: BTreeMap::new(),
            retry_files: BTreeSet::new(),
        }
    }

    pub fn from_save_state(save_state: SaveState<T>) -> Self {
        let cache
            = save_state
                .cache
                .into_iter()
                .map(|(path, entry)| {
                    let cache_entry
                        = match entry {
                            SaveEntry::File(mtime, data) => CacheEntry::File(mtime, data),
                            SaveEntry::Directory(mtime) => CacheEntry::Directory(mtime),
                        };
                    (path, cache_entry)
                })
                .collect();

        Self {
            cache,
            roots: save_state.roots,
            failed_files: BTreeMap::new(),
            retry_files: save_state.retry_files,
        }
    }

    pub fn to_save_state(&self) -> SaveState<T>
    where
        T: Clone,
    {
        let cache
            = self
                .cache
                .iter()
                .map(|(path, entry)| {
                    let save_entry
                        = match entry {
                            CacheEntry::File(mtime, data) => SaveEntry::File(*mtime, data.clone()),
                            CacheEntry::Directory(mtime) => SaveEntry::Directory(*mtime),
                        };
                    (path.clone(), save_entry)
                })
                .collect();

        let retry_files
            = self.failed_files.keys().cloned().collect();

        SaveState {
            cache,
            roots: self.roots.clone(),
            retry_files,
        }
    }
}

fn deduplicate_roots(roots: Vec<Path>) -> Vec<Path> {
    let mut sorted_roots
        = roots;

    sorted_roots.sort_by(|a, b| {
        a.as_str().len().cmp(&b.as_str().len())
    });

    let mut deduplicated
        = Vec::new();

    for root in sorted_roots {
        let is_covered
            = deduplicated.iter().any(|existing: &Path| {
                if root.as_str().is_empty() {
                    return false;
                }
                if existing.as_str().is_empty() {
                    return true;
                }

                let existing_str
                    = existing.as_str();
                let root_str
                    = root.as_str();

                root_str.starts_with(existing_str)
                    && root_str[existing_str.len()..].starts_with('/')
            });

        if !is_covered {
            deduplicated.push(root);
        }
    }

    deduplicated.sort();
    deduplicated
}

#[derive(Debug)]
pub struct DiffFinder<TController: DiffController> {
    pub root_path: Path,
    pub cache_state: CacheState<TController::Data>,
}

impl<TController: DiffController> DiffFinder<TController> {
    pub fn new(root_path: Path, roots: Vec<Path>, save_state: SaveState<TController::Data>) -> Self {
        let deduplicated_roots
            = deduplicate_roots(roots);

        let mut cache_state
            = if save_state.roots == deduplicated_roots {
                CacheState::from_save_state(save_state)
            } else {
                CacheState::new(deduplicated_roots)
            };

        // Ensure failed files from a previous run are marked for retry by resetting their mtime.
        // Since we don't persist errors, any file that previously failed will have been removed
        // from the cache, so no special handling is needed here.
        for root in &cache_state.roots {
            if !cache_state.cache.contains_key(root) {
                cache_state.cache.insert(root.clone(), CacheEntry::Directory(0));
            }
        }

        Self {
            root_path,
            cache_state,
        }
    }

    fn refresh_directory(
        &mut self,
        new_file_paths: &mut Vec<Path>,
        rel_path: &Path,
        current_time: u128,
    ) -> Result<(), Error> {
        self.cache_state.cache.insert(
            rel_path.clone(),
            CacheEntry::Directory(current_time),
        );

        let abs_path
            = self.root_path.with_join(rel_path);

        let directory_entries
            = abs_path
                .fs_read_dir()?
                .into_iter()
                .collect::<Result<Vec<_>, _>>()?;

        for entry in directory_entries {
            let file_type
                = entry.file_type()?;
            let file_name_os
                = entry.file_name();

            let Some(file_name) = file_name_os.to_str() else {
                continue;
            };

            if !TController::is_relevant_entry(file_name, &file_type) {
                continue;
            }

            let entry_rel_path
                = rel_path.with_join_str(entry.file_name().to_str().unwrap());

            if self.cache_state.cache.contains_key(&entry_rel_path) {
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
        let cache_checks
            = self
                .cache_state
                .cache
                .par_iter()
                .map(|(rel_path, cache_entry)| {
                    let abs_path
                        = self.root_path.with_join(&rel_path);

                    let metadata
                        = match abs_path.fs_metadata() {
                            Ok(metadata) => metadata,

                            Err(e) if e.io_kind() == Some(ErrorKind::NotFound) => {
                                return Ok(CacheCheck::NotFound(rel_path.clone()))
                            }

                            Err(e) => return Err(e.into()),
                        };

                    let mtime
                        = metadata
                            .modified()?
                            .duration_since(UNIX_EPOCH)?
                            .as_nanos() as u128;

                    if metadata.is_dir() {
                        if mtime > cache_entry.mtime() {
                            Ok(CacheCheck::ChangedDirectory(rel_path.clone(), mtime))
                        } else {
                            Ok(CacheCheck::Skip)
                        }
                    } else {
                        if mtime > cache_entry.mtime() {
                            let Some(file_name) = rel_path.basename() else {
                                return Ok(CacheCheck::Skip);
                            };

                            if !TController::is_relevant_entry(file_name, &metadata.file_type()) {
                                return Ok(CacheCheck::NotFound(rel_path.clone()));
                            }

                            let data_result
                                = TController::get_file_data(&abs_path, &metadata);

                            Ok(CacheCheck::ChangedFile(rel_path.clone(), mtime, data_result))
                        } else {
                            Ok(CacheCheck::Skip)
                        }
                    }
                })
                .collect::<Result<Vec<_>, Error>>()?;

        let mut has_changed
            = false;
        let mut new_file_paths
            = std::mem::take(&mut self.cache_state.retry_files)
                .into_iter()
                .collect::<Vec<_>>();
        let mut all_changed_paths
            = BTreeSet::new();

        for cache_check in cache_checks {
            match cache_check {
                CacheCheck::Skip => {}

                CacheCheck::ChangedFile(rel_path, mtime, data_result) => {
                    match data_result {
                        Ok(data) => {
                            self.cache_state.cache.insert(
                                rel_path.clone(),
                                CacheEntry::File(mtime, data),
                            );
                            self.cache_state.failed_files.remove(&rel_path);
                        }

                        Err(error) => {
                            self.cache_state.cache.remove(&rel_path);
                            self.cache_state.failed_files.insert(
                                rel_path.clone(),
                                FailedFile { mtime, error },
                            );
                        }
                    }

                    all_changed_paths.insert(rel_path);
                    has_changed = true;
                }

                CacheCheck::ChangedDirectory(rel_path, mtime) => {
                    self.refresh_directory(&mut new_file_paths, &rel_path, mtime)?;
                    has_changed = true;
                }

                CacheCheck::NotFound(rel_path) => {
                    self.cache_state.cache.remove(&rel_path);
                    self.cache_state.failed_files.remove(&rel_path);
                    has_changed = true;
                }
            }
        }

        let new_entries
            = new_file_paths
                .into_par_iter()
                .map(|rel_path| {
                    let abs_path
                        = self.root_path.with_join(&rel_path);

                    let metadata
                        = abs_path.fs_metadata()?;

                    let data_result
                        = TController::get_file_data(&abs_path, &metadata);

                    let mtime
                        = metadata
                            .modified()?
                            .duration_since(UNIX_EPOCH)?
                            .as_nanos() as u128;

                    Ok((rel_path, mtime, data_result))
                })
                .collect::<Result<Vec<_>, Error>>()?;

        for (rel_path, mtime, data_result) in new_entries {
            match data_result {
                Ok(data) => {
                    self.cache_state.cache.insert(
                        rel_path.clone(),
                        CacheEntry::File(mtime, data),
                    );
                }

                Err(error) => {
                    self.cache_state.failed_files.insert(
                        rel_path.clone(),
                        FailedFile { mtime, error },
                    );
                }
            }

            all_changed_paths.insert(rel_path);
        }

        Ok((has_changed, all_changed_paths))
    }

    pub fn into_cache_state(self) -> CacheState<TController::Data> {
        self.cache_state
    }

    pub fn to_save_state(&self) -> SaveState<TController::Data>
    where
        TController::Data: Clone,
    {
        self.cache_state.to_save_state()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> Path {
        Path::new().with_join_str(s)
    }

    #[test]
    fn test_deduplicate_roots_removes_inner_paths() {
        let roots
            = vec![
                p("packages/foo"),
                p("packages"),
                p("packages/foo/bar"),
            ];

        let result
            = deduplicate_roots(roots);

        assert_eq!(result, vec![p("packages")]);
    }

    #[test]
    fn test_deduplicate_roots_keeps_non_overlapping() {
        let roots
            = vec![
                p("packages"),
                p("apps"),
                p("libs"),
            ];

        let result
            = deduplicate_roots(roots);

        assert_eq!(
            result,
            vec![
                p("apps"),
                p("libs"),
                p("packages"),
            ]
        );
    }

    #[test]
    fn test_deduplicate_roots_handles_empty_root() {
        let roots
            = vec![
                p("packages"),
                Path::new(),
            ];

        let result
            = deduplicate_roots(roots);

        assert_eq!(result, vec![Path::new()]);
    }

    #[test]
    fn test_deduplicate_roots_partial_prefix_not_ancestor() {
        let roots
            = vec![
                p("pack"),
                p("packages"),
            ];

        let result
            = deduplicate_roots(roots);

        assert_eq!(
            result,
            vec![p("pack"), p("packages")]
        );
    }
}
