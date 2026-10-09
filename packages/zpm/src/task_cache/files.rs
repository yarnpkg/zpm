use std::{
    collections::{BTreeMap, HashMap},
    os::unix::fs::MetadataExt,
    sync::Mutex,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use rayon::iter::{
    IntoParallelIterator,
    ParallelIterator,
};
use rkyv::Archive;
use zpm_utils::{
    Hash64,
    Hash64Writer,
    IoResultExt,
    Path,
};

use super::patterns::PatternSet;
use crate::error::Error;

const STATE_VERSION: u32 = 1;

/// Files modified more recently than this aren't recorded in the state:
/// a write landing in the same timestamp granularity as our stat call could
/// otherwise go unnoticed ("racy git" problem).
const RACY_WINDOW: Duration = Duration::from_secs(2);

/// Folders never crawled when looking for input files.
pub const ALWAYS_IGNORED_FOLDERS: &[&str] = &[".git", ".yarn", "node_modules"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct FileStamp {
    pub mtime_ns: i128,
    pub size: u64,
    pub ino: u64,
    pub mode: u32,
}

impl FileStamp {
    pub fn from_metadata(metadata: &std::fs::Metadata) -> Self {
        let mtime_ns
            = metadata.mtime() as i128 * 1_000_000_000 + metadata.mtime_nsec() as i128;

        Self {
            mtime_ns,
            size: metadata.size(),
            ino: metadata.ino(),
            mode: metadata.mode(),
        }
    }

    fn is_racy(&self, now: SystemTime) -> bool {
        let now_ns
            = now.duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos() as i128;

        now_ns - self.mtime_ns < RACY_WINDOW.as_nanos() as i128
    }
}

#[derive(Debug, Clone, Archive, rkyv::Serialize, rkyv::Deserialize)]
struct StateEntry {
    path: String,
    stamp: FileStamp,
    hash: Hash64,
}

#[derive(Debug, Clone, Archive, rkyv::Serialize, rkyv::Deserialize)]
struct StateFile {
    version: u32,
    entries: Vec<StateEntry>,
}

/// The hash of a file, along with the bits of metadata that matter to us.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HashedFile {
    pub hash: Hash64,
    pub mode: u32,
}

/// Remembers the content hashes of the files we've seen, keyed by their
/// absolute path and validated by their (mtime, size, inode, mode). It's
/// what makes warm runs cheap: we only need to stat the files rather than
/// read them.
pub struct FileHashState {
    path: Path,
    // Sharded by path: tasks hash their inputs from many threads at once,
    // and a single lock made warm runs spend most of their time waiting
    shards: Vec<Mutex<HashMap<String, (FileStamp, Hash64)>>>,
    dirty: std::sync::atomic::AtomicBool,
}

const STATE_SHARDS: usize = 64;

fn shard_index(path: &str) -> usize {
    use std::hash::{Hash, Hasher};

    let mut hasher
        = std::collections::hash_map::DefaultHasher::new();

    path.hash(&mut hasher);

    hasher.finish() as usize % STATE_SHARDS
}

impl FileHashState {
    pub fn load(path: Path) -> Self {
        let entries: Vec<StateEntry> = path.fs_read()
            .ok()
            .and_then(|data| rkyv::from_bytes::<StateFile, rkyv::rancor::BoxedError>(&data).ok())
            .filter(|state| state.version == STATE_VERSION)
            .map(|state| state.entries)
            .unwrap_or_default();

        let mut shards: Vec<HashMap<String, (FileStamp, Hash64)>>
            = (0..STATE_SHARDS).map(|_| HashMap::new()).collect();

        for entry in entries {
            shards[shard_index(&entry.path)].insert(entry.path, (entry.stamp, entry.hash));
        }

        Self {
            path,
            shards: shards.into_iter().map(Mutex::new).collect(),
            dirty: std::sync::atomic::AtomicBool::new(false),
        }
    }

    pub fn len(&self) -> usize {
        self.shards.iter().map(|shard| shard.lock().unwrap().len()).sum()
    }

    pub fn save_if_dirty(&self) -> Result<(), Error> {
        if !self.dirty.swap(false, std::sync::atomic::Ordering::AcqRel) {
            return Ok(());
        }

        let entries = self.shards.iter()
            .flat_map(|shard| shard.lock().unwrap().iter()
                .map(|(path, (stamp, hash))| StateEntry {path: path.clone(), stamp: *stamp, hash: hash.clone()})
                .collect::<Vec<_>>())
            .collect();

        let data
            = rkyv::to_bytes::<rkyv::rancor::BoxedError>(&StateFile {version: STATE_VERSION, entries})
                .map_err(|err| Error::TaskCacheError(format!("Failed to serialize the file hash state: {}", err)))?;

        self.path.fs_create_parent()?;
        self.path.fs_write_atomic(|tmp_path| tmp_path.fs_write(&data).map(|_| ()))?;

        Ok(())
    }

    pub fn clear(&self) {
        for shard in &self.shards {
            shard.lock().unwrap().clear();
        }

        self.dirty.store(false, std::sync::atomic::Ordering::Release);
    }

    fn lookup(&self, abs_path: &str, stamp: &FileStamp) -> Option<Hash64> {
        let entries
            = self.shards[shard_index(abs_path)].lock().unwrap();

        entries.get(abs_path)
            .filter(|(known_stamp, _)| known_stamp == stamp)
            .map(|(_, hash)| hash.clone())
    }

    /// Records the hash of a file we just wrote (or just read).
    pub fn record(&self, abs_path: String, stamp: FileStamp, hash: Hash64, now: SystemTime) {
        if stamp.is_racy(now) {
            return;
        }

        self.shards[shard_index(&abs_path)].lock().unwrap().insert(abs_path, (stamp, hash));
        self.dirty.store(true, std::sync::atomic::Ordering::Release);
    }

    /// Hashes a single file (or symlink, in which case we hash its target
    /// path rather than following it). Returns `None` if it doesn't exist.
    pub fn hash_file(&self, abs_path: &Path, now: SystemTime) -> Result<Option<HashedFile>, Error> {
        let Some(metadata) = abs_path.fs_symlink_metadata().ok_missing()? else {
            return Ok(None);
        };

        let stamp
            = FileStamp::from_metadata(&metadata);

        let key
            = abs_path.as_str();

        if let Some(hash) = self.lookup(key, &stamp) {
            return Ok(Some(HashedFile {hash, mode: stamp.mode}));
        }

        let hash = if metadata.file_type().is_symlink() {
            let target
                = abs_path.fs_read_link()?;

            hash_content(b"symlink", target.as_str().as_bytes())
        } else {
            let Some(data) = abs_path.fs_read().ok_missing()? else {
                return Ok(None);
            };

            hash_content(b"file", &data)
        };

        self.record(key.to_string(), stamp, hash.clone(), now);

        Ok(Some(HashedFile {hash, mode: stamp.mode}))
    }

    /// Hashes a set of files in parallel. The paths are relative to `base`.
    pub fn hash_files(&self, base: &Path, rel_paths: Vec<String>) -> Result<BTreeMap<String, HashedFile>, Error> {
        let now
            = SystemTime::now();

        let results: Vec<Result<Option<(String, HashedFile)>, Error>> = rel_paths.into_par_iter()
            .map(|rel_path| {
                let hashed
                    = self.hash_file(&base.with_join_str(&rel_path), now)?;

                Ok(hashed.map(|hashed| (rel_path, hashed)))
            })
            .collect();

        let mut files
            = BTreeMap::new();

        for result in results {
            if let Some((rel_path, hashed)) = result? {
                files.insert(rel_path, hashed);
            }
        }

        Ok(files)
    }
}

pub fn hash_content(kind: &[u8], data: &[u8]) -> Hash64 {
    let mut writer
        = Hash64Writer::new();

    writer.update(kind);
    writer.update([0]);
    writer.update(data);

    writer.finalize()
}

pub struct WalkOptions<'a> {
    /// Whether to honor the `.gitignore` files (including those in the
    /// parent folders, up to the repository root).
    pub gitignore: bool,

    /// Folders whose name is listed here are never crawled.
    pub ignored_folder_names: &'a [&'a str],

    /// Files (relative to the base) excluded from the results.
    pub excluded_files: &'a [&'a str],

    /// Folders (absolute paths) that are never crawled; used to avoid
    /// listing the files of nested workspaces.
    pub excluded_folders: &'a [Path],
}

/// Lists the files under `base` matching the given pattern set (or all the
/// files if `patterns` is `None`), sorted by path. Returned paths are
/// relative to `base` and use forward slashes.
pub fn list_files(base: &Path, patterns: Option<&PatternSet>, excluded: Option<&PatternSet>, options: &WalkOptions) -> Result<Vec<String>, Error> {
    let roots = match patterns {
        Some(patterns) => patterns.roots().to_vec(),
        None => vec![String::new()],
    };

    let mut files
        = Vec::new();

    for root in roots {
        let root_path
            = base.with_join_str(&root);

        let Some(metadata) = root_path.fs_symlink_metadata().ok_missing()? else {
            continue;
        };

        // A root that's a symlink to a folder (`src -> ../shared/src`) is
        // walked like a folder, otherwise `src/**` would match nothing
        let is_dir = metadata.is_dir()
            || (metadata.file_type().is_symlink() && root_path.fs_is_dir());

        if !is_dir {
            files.push(root.clone());
            continue;
        }

        let mut builder
            = ignore::WalkBuilder::new(root_path.to_path_buf());

        builder
            .standard_filters(false)
            .hidden(false)
            .follow_links(false)
            .git_ignore(options.gitignore)
            .git_exclude(options.gitignore)
            .git_global(options.gitignore)
            .parents(options.gitignore)
            .require_git(true)
            .sort_by_file_name(|a, b| a.cmp(b));

        let ignored_folder_names: Vec<String>
            = options.ignored_folder_names.iter().map(|name| name.to_string()).collect();

        let excluded_folders: std::collections::HashSet<std::path::PathBuf>
            = options.excluded_folders.iter().map(|path| path.to_path_buf()).collect();

        // Patterns excluding a whole folder (`!**/.venv/**`) prune it from
        // the walk instead of filtering its files afterwards
        let pruning_patterns
            = [patterns.cloned(), excluded.cloned()];

        let walk_base
            = base.to_path_buf();

        builder.filter_entry(move |entry| {
            let is_dir
                = entry.file_type().map_or(false, |file_type| file_type.is_dir());

            if !is_dir || entry.depth() == 0 {
                return true;
            }

            if entry.file_name().to_str().map_or(false, |name| ignored_folder_names.iter().any(|ignored| ignored == name)) {
                return false;
            }

            if excluded_folders.contains(entry.path()) {
                return false;
            }

            let Some(rel_path) = entry.path().strip_prefix(&walk_base).ok().and_then(|path| path.to_str()) else {
                return true;
            };

            let folder_marker
                = format!("{}/\0", rel_path);

            let [patterns, excluded] = &pruning_patterns;

            let is_pruned_by_patterns
                = patterns.as_ref().map_or(false, |patterns| patterns.is_excluded(&folder_marker));
            let is_pruned_by_excluded
                = excluded.as_ref().map_or(false, |excluded| excluded.includes_folder(&folder_marker));

            !is_pruned_by_patterns && !is_pruned_by_excluded
        });

        for entry in builder.build() {
            let entry = entry
                .map_err(|err| Error::TaskCacheError(format!("Failed to crawl {}: {}", root_path.as_str(), err)))?;

            if entry.file_type().map_or(true, |file_type| file_type.is_dir()) {
                continue;
            }

            let abs_path
                = Path::try_from(entry.path())?;

            files.push(abs_path.relative_to(base).as_str().to_string());
        }
    }

    files.retain(|rel_path| {
        if options.excluded_files.contains(&rel_path.as_str()) {
            return false;
        }

        if let Some(patterns) = patterns {
            if !patterns.is_match(rel_path) {
                return false;
            }
        }

        if let Some(excluded) = excluded {
            if excluded.is_match(rel_path) {
                return false;
            }
        }

        true
    });

    files.sort();
    files.dedup();

    Ok(files)
}
