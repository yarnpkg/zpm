use std::sync::{Arc, LazyLock};

use bytes::Bytes;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::OnceCell;
use zpm_utils::Path;

use crate::error::Error;
use crate::project::Project;

const SCHEMA_VERSION: u32 = 1;
const MANIFEST_CACHE_DIR: &str = "manifest";

#[derive(Debug, Clone)]
pub struct ManifestCacheEntry {
    pub body: Bytes,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub fresh_until: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct ManifestCacheMeta {
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub fresh_until: Option<u64>,
}

impl ManifestCacheMeta {
    fn from_entry(entry: &ManifestCacheEntry) -> Self {
        Self {
            etag: entry.etag.clone(),
            last_modified: entry.last_modified.clone(),
            fresh_until: entry.fresh_until,
        }
    }
}

#[derive(Debug)]
pub struct ManifestCache {
    root: Path,
    enable_write: bool,
    enabled: bool,
    enable_cache_control: bool,
    max_age: std::time::Duration,
}

#[derive(Debug, Serialize, Deserialize)]
struct CacheMeta {
    version: u32,
    etag: Option<String>,
    last_modified: Option<String>,
    fresh_until: Option<u64>,
}

static MEMORY_ENTRIES: LazyLock<DashMap<String, Arc<ManifestCacheEntry>>> = LazyLock::new(DashMap::new);
static MEMORY_META: LazyLock<DashMap<String, ManifestCacheMeta>> = LazyLock::new(DashMap::new);
static IN_FLIGHT: LazyLock<DashMap<String, Arc<OnceCell<Result<Bytes, Error>>>>> = LazyLock::new(DashMap::new);

impl ManifestCache {
    pub fn new(project: &Project) -> Result<Self, Error> {
        let enabled = project.config.settings.enable_manifest_cache.value;
        let enable_cache_control = project.config.settings.enable_manifest_cache_control.value;
        let max_age = project.config.settings.manifest_cache_max_age.value;
        let root = project.preferred_cache_path()
            .with_join_str(MANIFEST_CACHE_DIR);

        let enable_write = enabled && !project.config.settings.enable_immutable_cache.value;

        if enable_write {
            root.fs_create_dir_all()?;
        }

        Ok(Self {
            root,
            enable_write,
            enabled,
            enable_cache_control,
            max_age,
        })
    }

    pub fn cache_key(registry: &str, path: &str) -> String {
        format!("{}{}", registry, path)
    }

    pub fn get_meta(&self, key: &str) -> Result<Option<ManifestCacheMeta>, Error> {
        if !self.enabled {
            return Ok(None);
        }

        if let Some(entry) = MEMORY_ENTRIES.get(key) {
            return Ok(Some(ManifestCacheMeta::from_entry(entry.as_ref())));
        }

        if let Some(meta) = MEMORY_META.get(key) {
            return Ok(Some(meta.clone()));
        }

        let (_body_path, meta_path) = self.paths_for_key(key);

        if !meta_path.fs_exists() {
            return Ok(None);
        }

        let meta_text = match meta_path.fs_read_text() {
            Ok(text) => text,
            Err(_) => return Ok(None),
        };

        let meta: CacheMeta = match serde_json::from_str(&meta_text) {
            Ok(meta) => meta,
            Err(_) => return Ok(None),
        };

        if meta.version != SCHEMA_VERSION {
            return Ok(None);
        }

        let meta = ManifestCacheMeta {
            etag: meta.etag,
            last_modified: meta.last_modified,
            fresh_until: meta.fresh_until,
        };

        MEMORY_META.insert(key.to_string(), meta.clone());

        Ok(Some(meta))
    }

    pub fn get_entry(&self, key: &str, meta: Option<&ManifestCacheMeta>) -> Result<Option<Arc<ManifestCacheEntry>>, Error> {
        if !self.enabled {
            return Ok(None);
        }

        if let Some(entry) = MEMORY_ENTRIES.get(key) {
            return Ok(Some(entry.clone()));
        }

        let meta = match meta {
            Some(meta) => meta.clone(),
            None => match self.get_meta(key)? {
                Some(meta) => meta,
                None => return Ok(None),
            },
        };

        let (body_path, _) = self.paths_for_key(key);

        let body = match body_path.fs_read() {
            Ok(body) => body,
            Err(_) => return Ok(None),
        };

        let entry = Arc::new(ManifestCacheEntry {
            body: Bytes::from(body),
            etag: meta.etag.clone(),
            last_modified: meta.last_modified.clone(),
            fresh_until: meta.fresh_until,
        });

        MEMORY_ENTRIES.insert(key.to_string(), entry.clone());
        MEMORY_META.insert(key.to_string(), meta);

        Ok(Some(entry))
    }

    pub fn put(&self, key: &str, entry: &ManifestCacheEntry) -> Result<(), Error> {
        if !self.enable_write {
            return Ok(());
        }

        let (body_path, meta_path) = self.paths_for_key(key);
        let hash = hash_key(key);

        let fresh_until = entry.fresh_until.or_else(|| self.compute_fresh_until());
        let updated_entry = ManifestCacheEntry {
            body: entry.body.clone(),
            etag: entry.etag.clone(),
            last_modified: entry.last_modified.clone(),
            fresh_until,
        };

        let meta = CacheMeta {
            version: SCHEMA_VERSION,
            etag: updated_entry.etag.clone(),
            last_modified: updated_entry.last_modified.clone(),
            fresh_until,
        };

        let meta_text = serde_json::to_string(&meta)
            .map_err(|err| Error::SerializationError(err.to_string()))?;

        let tmp_body = self.root.with_join_str(format!(".{}.body.tmp-{}", hash, rand::random::<u64>()));
        let tmp_meta = self.root.with_join_str(format!(".{}.meta.tmp-{}", hash, rand::random::<u64>()));

        tmp_body.fs_write(updated_entry.body.as_ref())?;
        tmp_body.fs_rename(&body_path)?;

        tmp_meta.fs_write_text(meta_text)?;
        tmp_meta.fs_rename(&meta_path)?;

        MEMORY_ENTRIES.insert(key.to_string(), Arc::new(updated_entry.clone()));
        MEMORY_META.insert(key.to_string(), ManifestCacheMeta::from_entry(&updated_entry));

        Ok(())
    }

    pub fn refresh(&self, key: &str, entry: &ManifestCacheEntry) -> Result<(), Error> {
        if !self.enable_write {
            return Ok(());
        }

        let (_body_path, meta_path) = self.paths_for_key(key);
        let hash = hash_key(key);

        let fresh_until = self.compute_fresh_until();
        let updated_entry = ManifestCacheEntry {
            body: entry.body.clone(),
            etag: entry.etag.clone(),
            last_modified: entry.last_modified.clone(),
            fresh_until,
        };

        let meta = CacheMeta {
            version: SCHEMA_VERSION,
            etag: updated_entry.etag.clone(),
            last_modified: updated_entry.last_modified.clone(),
            fresh_until,
        };

        let meta_text = serde_json::to_string(&meta)
            .map_err(|err| Error::SerializationError(err.to_string()))?;

        let tmp_meta = self.root.with_join_str(format!(".{}.meta.tmp-{}", hash, rand::random::<u64>()));
        tmp_meta.fs_write_text(meta_text)?;
        tmp_meta.fs_rename(&meta_path)?;

        MEMORY_META.insert(key.to_string(), ManifestCacheMeta::from_entry(&updated_entry));
        MEMORY_ENTRIES.insert(key.to_string(), Arc::new(updated_entry));

        Ok(())
    }

    pub fn is_fresh_meta(&self, meta: &ManifestCacheMeta) -> bool {
        if !self.enable_cache_control || self.max_age.is_zero() {
            return false;
        }

        let Some(fresh_until) = meta.fresh_until else {
            return false;
        };

        fresh_until >= now_seconds()
    }

    pub fn in_flight_cell(&self, key: &str) -> Arc<OnceCell<Result<Bytes, Error>>> {
        IN_FLIGHT.entry(key.to_string())
            .or_insert_with(|| Arc::new(OnceCell::new()))
            .clone()
    }

    pub fn clear_in_flight(&self, key: &str) {
        IN_FLIGHT.remove(key);
    }

    fn paths_for_key(&self, key: &str) -> (Path, Path) {
        let hash = hash_key(key);
        let body_path = self.root.with_join_str(format!("{}.json", hash));
        let meta_path = self.root.with_join_str(format!("{}.meta.json", hash));
        (body_path, meta_path)
    }

    fn compute_fresh_until(&self) -> Option<u64> {
        if !self.enable_cache_control || self.max_age.is_zero() {
            return None;
        }

        Some(now_seconds().saturating_add(self.max_age.as_secs()))
    }
}

fn hash_key(key: &str) -> String {
    hex::encode(Sha256::digest(key.as_bytes()))
}

fn now_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
