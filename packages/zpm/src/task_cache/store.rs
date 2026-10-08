use std::{
    borrow::Cow,
    os::unix::fs::PermissionsExt,
    str::FromStr,
    time::SystemTime,
};

use serde::{
    Deserialize,
    Serialize,
};
use zpm_formats::{
    CompressionAlgorithm,
    Entry,
    zip::{
        ToZip,
        entries_from_zip,
    },
};
use zpm_utils::{
    FromFileString,
    Hash64,
    IoResultExt,
    Path,
    ToFileString,
};

use super::files::{
    FileHashState,
    FileStamp,
    hash_content,
};
use crate::error::Error;

const META_ENTRY_NAME: &str = "meta.json";
const OUTPUTS_PREFIX: &str = "outputs/";

const S_IFMT: u32 = 0o170000;
const S_IFLNK: u32 = 0o120000;
const S_IFREG: u32 = 0o100000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogLine {
    pub stream: String,
    pub line: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutputFile {
    pub path: String,
    pub hash: Hash64,
    /// Full `st_mode` (file type and permission bits)
    pub mode: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryMeta {
    pub format: String,
    pub task: String,
    pub fingerprint: Hash64,
    pub created_at: u64,
    pub logs: Vec<LogLine>,
    pub outputs: Vec<OutputFile>,
}

pub fn entry_path(cache_folder: &Path, fingerprint: &Hash64) -> Path {
    let hex
        = fingerprint.to_file_string();

    cache_folder
        .with_join_str(&hex[0..2])
        .with_join_str(format!("{}.zip", hex))
}

fn is_safe_rel_path(rel_path: &str) -> bool {
    !rel_path.is_empty()
        && !rel_path.starts_with('/')
        && rel_path.split('/').all(|segment| !segment.is_empty() && segment != "." && segment != "..")
}

pub struct CollectedOutput {
    pub file: OutputFile,
    pub data: Vec<u8>,
}

/// Reads the output files from the disk so they can be stored.
pub fn collect_outputs(base: &Path, rel_paths: &[String]) -> Result<Vec<CollectedOutput>, Error> {
    let mut outputs
        = Vec::with_capacity(rel_paths.len());

    for rel_path in rel_paths {
        if !is_safe_rel_path(rel_path) {
            return Err(Error::TaskCacheError(format!("Output paths must be within the workspace (got {})", rel_path)));
        }

        let abs_path
            = base.with_join_str(rel_path);

        let Some(metadata) = abs_path.fs_symlink_metadata().ok_missing()? else {
            continue;
        };

        let mode
            = metadata.permissions().mode();

        let (data, kind): (Vec<u8>, &[u8]) = if metadata.file_type().is_symlink() {
            (abs_path.fs_read_link()?.as_str().as_bytes().to_vec(), b"symlink")
        } else {
            (abs_path.fs_read()?, b"file")
        };

        outputs.push(CollectedOutput {
            file: OutputFile {
                path: rel_path.clone(),
                hash: hash_content(kind, &data),
                mode,
            },
            data,
        });
    }

    Ok(outputs)
}

/// Atomically writes a cache entry; concurrent writers of the same entry
/// are fine, since they'd both write equivalent content.
pub fn write_entry(cache_folder: &Path, meta: &EntryMeta, outputs: Vec<CollectedOutput>) -> Result<u64, Error> {
    let meta_json
        = serde_json::to_vec(meta)
            .map_err(|err| Error::TaskCacheError(format!("Failed to serialize the cache entry metadata: {}", err)))?;

    let mut entries
        = Vec::with_capacity(outputs.len() + 1);

    entries.push(Entry::new_file(Path::from_str(META_ENTRY_NAME).unwrap(), Cow::Owned(meta_json)));

    for output in outputs {
        let name
            = Path::from_file_string(&format!("{}{}", OUTPUTS_PREFIX, output.file.path))?;

        let mut entry
            = Entry::new_file(name, Cow::Owned(output.data));

        entry.mode = output.file.mode;
        entries.push(entry);
    }

    for entry in entries.iter_mut() {
        entry.compress_in_place(CompressionAlgorithm::Deflate(1));
    }

    let data
        = entries.to_zip();

    let path
        = entry_path(cache_folder, &meta.fingerprint);

    path.fs_create_parent()?;
    path.fs_write_atomic(|tmp_path| tmp_path.fs_write(&data).map(|_| ()))?;

    Ok(data.len() as u64)
}

pub struct CachedEntry {
    pub meta: EntryMeta,
    data: Vec<u8>,
}

pub fn read_entry(cache_folder: &Path, fingerprint: &Hash64) -> Result<Option<CachedEntry>, Error> {
    let path
        = entry_path(cache_folder, fingerprint);

    let Some(data) = path.fs_read().ok_missing()? else {
        return Ok(None);
    };

    let meta = {
        let entries
            = entries_from_zip(&data)?;

        let meta_entry = entries.iter()
            .find(|entry| entry.name.as_str() == META_ENTRY_NAME)
            .ok_or_else(|| Error::TaskCacheError(format!("Corrupted cache entry: {}", path.as_str())))?;

        serde_json::from_slice::<EntryMeta>(&meta_entry.data)
            .map_err(|err| Error::TaskCacheError(format!("Corrupted cache entry {}: {}", path.as_str(), err)))?
    };

    if &meta.fingerprint != fingerprint {
        return Err(Error::TaskCacheError(format!("Corrupted cache entry: {} (fingerprint mismatch)", path.as_str())));
    }

    Ok(Some(CachedEntry {meta, data}))
}

fn prune_empty_parents(base: &Path, rel_paths: &[String]) {
    let mut folders: Vec<String> = rel_paths.iter()
        .flat_map(|rel_path| {
            let segments: Vec<&str>
                = rel_path.split('/').collect();

            (1..segments.len()).map(move |n| segments[..n].join("/"))
        })
        .collect();

    // Deepest folders first, so that their parents may become empty
    folders.sort_by(|a, b| b.matches('/').count().cmp(&a.matches('/').count()).then(b.cmp(a)));
    folders.dedup();

    for folder in folders {
        let _ = std::fs::remove_dir(base.with_join_str(&folder).to_path_buf());
    }
}

impl CachedEntry {
    /// Whether the files currently on disk are exactly the ones stored in
    /// the entry, in which case there's no need to restore anything.
    pub fn matches_disk(&self, base: &Path, current: &[String], file_state: &FileHashState) -> Result<bool, Error> {
        if current.len() != self.meta.outputs.len() {
            return Ok(false);
        }

        let now
            = SystemTime::now();

        for (rel_path, output) in current.iter().zip(self.meta.outputs.iter()) {
            if *rel_path != output.path {
                return Ok(false);
            }

            let Some(hashed) = file_state.hash_file(&base.with_join_str(rel_path), now)? else {
                return Ok(false);
            };

            if hashed.hash != output.hash || hashed.mode != output.mode {
                return Ok(false);
            }
        }

        Ok(true)
    }

    /// Removes the stale outputs (the files currently matching the output
    /// globs) and replaces them by the ones stored in the entry.
    pub fn restore(&self, base: &Path, current: &[String], file_state: &FileHashState) -> Result<(), Error> {
        for rel_path in current {
            base.with_join_str(rel_path)
                .fs_rm_file()
                .ok_missing()?;
        }

        prune_empty_parents(base, current);

        let entries
            = entries_from_zip(&self.data)?;

        let now
            = SystemTime::now();

        for entry in entries {
            let Some(rel_path) = entry.name.as_str().strip_prefix(OUTPUTS_PREFIX) else {
                continue;
            };

            if !is_safe_rel_path(rel_path) {
                return Err(Error::TaskCacheError(format!("Refusing to restore unsafe path {}", rel_path)));
            }

            let abs_path
                = base.with_join_str(rel_path);

            abs_path.fs_create_parent()?;

            // A folder may sit where the file needs to go
            if abs_path.fs_is_real_dir() {
                abs_path.fs_rm()?;
            }

            let file_type
                = entry.mode & S_IFMT;

            if file_type == S_IFLNK {
                let target
                    = std::str::from_utf8(&entry.data)
                        .map_err(|_| Error::TaskCacheError(format!("Invalid symlink target for {}", rel_path)))?;

                std::os::unix::fs::symlink(target, abs_path.to_path_buf())?;
            } else if file_type == S_IFREG || file_type == 0 {
                abs_path.fs_write(&entry.data)?;
                abs_path.fs_set_permissions(std::fs::Permissions::from_mode(entry.mode & 0o7777))?;
            } else {
                return Err(Error::TaskCacheError(format!("Unsupported file type for {}", rel_path)));
            }

            if let Ok(metadata) = abs_path.fs_symlink_metadata() {
                let kind: &[u8]
                    = if file_type == S_IFLNK {b"symlink"} else {b"file"};

                file_state.record(abs_path.as_str().to_string(), FileStamp::from_metadata(&metadata), hash_content(kind, &entry.data), now);
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_safe_paths() {
        assert!(is_safe_rel_path("dist/index.js"));
        assert!(!is_safe_rel_path("../index.js"));
        assert!(!is_safe_rel_path("dist/../../index.js"));
        assert!(!is_safe_rel_path("/etc/passwd"));
        assert!(!is_safe_rel_path(""));
    }
}
