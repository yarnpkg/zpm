use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Debug,
    fs::{FileType, Metadata},
    io::ErrorKind,
    time::UNIX_EPOCH,
};

use rayon::iter::{IntoParallelIterator, IntoParallelRefIterator, ParallelIterator};
use rkyv::{Archive, with::Skip};
use zpm_utils::Path;

use crate::error::Error;

#[derive(Debug)]
enum CacheCheck<T> {
    Skip,
    NotFound(Path),
    ChangedFile(Path, u128, Result<T, Error>),
    ChangedDirectory(Path, u128),
}

#[derive(Debug, Clone, Archive, rkyv::Serialize, rkyv::Deserialize)]
#[rkyv(serialize_bounds(__S: rkyv::ser::Writer + rkyv::ser::Allocator + rkyv::ser::Sharing, T: rkyv::Serialize<__S>, <__S as rkyv::rancor::Fallible>::Error: rkyv::rancor::Source))]
#[rkyv(deserialize_bounds(__D: rkyv::de::Pooling, T::Archived: rkyv::Deserialize<T, __D>, <__D as rkyv::rancor::Fallible>::Error: rkyv::rancor::Source))]
#[rkyv(bytecheck(bounds(__C: rkyv::validation::ArchiveContext + rkyv::validation::SharedContext, T::Archived: rkyv::bytecheck::CheckBytes<__C>, <__C as rkyv::rancor::Fallible>::Error: rkyv::rancor::Source)))]
pub enum CacheEntry<T: Archive> {
    File(u128, #[rkyv(omit_bounds)] T),
    Error(#[rkyv(with = Skip)] Option<Error>),
    Directory(u128),
}

impl<T: Archive> CacheEntry<T> {
    pub fn mtime(&self) -> u128 {
        match self {
            CacheEntry::File(mtime, _) => *mtime,
            CacheEntry::Directory(mtime) => *mtime,
            CacheEntry::Error(_) => 0, // Errors will be retried
        }
    }
}

#[derive(Default, Debug, Archive, rkyv::Serialize, rkyv::Deserialize)]
#[rkyv(serialize_bounds(__S: rkyv::ser::Writer + rkyv::ser::Allocator + rkyv::ser::Sharing, T: rkyv::Serialize<__S>, <__S as rkyv::rancor::Fallible>::Error: rkyv::rancor::Source))]
#[rkyv(deserialize_bounds(__D: rkyv::de::Pooling, T::Archived: rkyv::Deserialize<T, __D>, <__D as rkyv::rancor::Fallible>::Error: rkyv::rancor::Source))]
#[rkyv(bytecheck(bounds(__C: rkyv::validation::ArchiveContext + rkyv::validation::SharedContext, T::Archived: rkyv::bytecheck::CheckBytes<__C>, <__C as rkyv::rancor::Fallible>::Error: rkyv::rancor::Source)))]
pub struct CacheState<T: Archive> {
    #[rkyv(omit_bounds)]
    pub cache: BTreeMap<Path, CacheEntry<T>>,
    pub roots: Vec<Path>,
}

impl<T: Archive> CacheState<T> {
    pub fn new(roots: Vec<Path>) -> Self {
        let cache
            = roots.iter()
                .map(|root| (root.clone(), CacheEntry::Directory(0)))
                .collect();

        Self {
            cache,
            roots,
        }
    }

    pub fn from_slice(data: &[u8]) -> Result<Self, Error>
    where
        T::Archived: rkyv::Deserialize<T, rkyv::rancor::Strategy<rkyv::de::Pool, rkyv::rancor::BoxedError>>,
        for<'a> <Self as Archive>::Archived: rkyv::bytecheck::CheckBytes<rkyv::api::high::HighValidator<'a, rkyv::rancor::BoxedError>>,
    {
        rkyv::from_bytes::<Self, rkyv::rancor::BoxedError>(data)
            .map_err(|_| Error::ReplaceMe)
    }

    pub fn to_vec(&self) -> Result<Vec<u8>, Error>
    where
        T: for<'a> rkyv::Serialize<rkyv::rancor::Strategy<rkyv::ser::Serializer<rkyv::util::AlignedVec, rkyv::ser::allocator::ArenaHandle<'a>, rkyv::ser::sharing::Share>, rkyv::rancor::BoxedError>>,
    {
        rkyv::to_bytes::<rkyv::rancor::BoxedError>(self)
            .map(|v| v.to_vec())
            .map_err(|_| Error::ReplaceMe)
    }
}

pub trait DiffController {
    type Data: Debug + Send + Sync + Archive;

    fn is_relevant_entry(file_name: &str, file_type: &FileType) -> bool;
    fn get_file_data(path: &Path, metadata: &Metadata) -> Result<Self::Data, Error>;
}

fn deduplicate_roots(mut roots: Vec<Path>) -> Vec<Path> {
    roots.sort_by(|a, b| {
        a.as_str().len().cmp(&b.as_str().len())
    });

    let mut deduplicated
        = Vec::new();

    for root in roots {
        let is_covered
            = deduplicated.iter()
                .any(|existing: &Path| existing.contains(&root));

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
    pub state: CacheState<TController::Data>,
}

impl<TController: DiffController> DiffFinder<TController> {
    pub fn new(root_path: Path, roots: Vec<Path>, state: CacheState<TController::Data>) -> Self {
        let deduplicated_roots
            = deduplicate_roots(roots);

        let mut state = if state.roots == deduplicated_roots {
            state
        } else {
            CacheState::new(deduplicated_roots)
        };

        for root in &state.roots {
            if !state.cache.contains_key(root) {
                state.cache.insert(root.clone(), CacheEntry::Directory(0));
            }
        }

        Self {root_path, state}
    }

    fn refresh_directory(
        &mut self,
        new_file_paths: &mut Vec<Path>,
        rel_path: &Path,
        current_time: u128,
    ) -> Result<(), Error> {
        self.state.cache.insert(
            rel_path.clone(),
            CacheEntry::Directory(current_time),
        );

        let abs_path
            = self.root_path
                .with_join(rel_path);

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
                = rel_path
                    .with_join_str(entry.file_name().to_str().unwrap());

            if self.state.cache.contains_key(&entry_rel_path) {
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
        let cache_checks = self.state
            .cache
            .par_iter()
            .map(|(rel_path, cache_entry)| {
                let abs_path
                    = self.root_path.with_join(&rel_path);

                let metadata = match abs_path.fs_metadata() {
                    Ok(metadata)
                        => metadata,

                    Err(e) if e.io_kind() == Some(ErrorKind::NotFound)
                        => return Ok(CacheCheck::NotFound(rel_path.clone())),

                    Err(e)
                        => return Err(e.into()),
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
            = vec![];
        let mut all_changed_paths
            = BTreeSet::new();

        for cache_check in cache_checks {
            match cache_check {
                CacheCheck::Skip => {}

                CacheCheck::ChangedFile(rel_path, mtime, data_result) => {
                    match data_result {
                        Ok(data) => {
                            self.state.cache.insert(
                                rel_path.clone(),
                                CacheEntry::File(mtime, data),
                            );
                        }

                        Err(error) => {
                            self.state.cache.insert(
                                rel_path.clone(),
                                CacheEntry::Error(Some(error)),
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
                    self.state.cache.remove(&rel_path);
                    has_changed = true;
                }
            }
        }

        let new_entries = new_file_paths
            .into_par_iter()
            .map(|rel_path| {
                let abs_path
                    = self.root_path.with_join(&rel_path);

                let metadata
                    = abs_path.fs_metadata()?;

                let data_result
                    = TController::get_file_data(&abs_path, &metadata);

                let mtime = metadata
                    .modified()?
                    .duration_since(UNIX_EPOCH)?
                    .as_nanos() as u128;

                Ok((rel_path, mtime, data_result))
            })
            .collect::<Result<Vec<_>, Error>>()?;

        for (rel_path, mtime, data_result) in new_entries {
            match data_result {
                Ok(data) => {
                    self.state.cache.insert(
                        rel_path.clone(),
                        CacheEntry::File(mtime, data),
                    );
                }

                Err(error) => {
                    self.state.cache.insert(
                        rel_path.clone(),
                        CacheEntry::Error(Some(error)),
                    );
                }
            }

            all_changed_paths.insert(rel_path);
        }

        Ok((has_changed, all_changed_paths))
    }

    pub fn into_state(self) -> CacheState<TController::Data> {
        self.state
    }
}
