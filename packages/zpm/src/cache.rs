use std::path::PathBuf;
use std::sync::Arc;
use std::collections::HashSet;
use std::sync::Mutex;
use itertools::Itertools;
use zpm_formats::{zip::ToZip, Entry};
use zpm_macro_enum::zpm_enum;
use zpm_primitives::Locator;
use zpm_utils::{Hash64, IoResultExt, Path, PathError, ToFileString};
use futures::Future;

use crate::error::Error;

pub const CACHE_VERSION: usize = 2;

#[zpm_enum]
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[derive_variants(Clone, Debug, PartialEq, Eq, Hash)]
pub enum CacheEntry {
    #[no_pattern]
    Info {
        path: Path,
        checksum: Option<Hash64>,
    },

    #[no_pattern]
    Data {
        info: InfoCacheEntry,
        data: Vec<u8>,
    },
}

impl CacheEntry {
    pub fn into_info(self) -> InfoCacheEntry {
        match self {
            CacheEntry::Info(params) => params,
            CacheEntry::Data(params) => params.info,
        }
    }
}

pub struct CachePacker {
    compression_algorithm: Option<zpm_formats::CompressionAlgorithm>,
}

impl CachePacker {
    pub fn pack<'a>(&self, mut entries: Vec<Entry<'a>>) -> Result<Vec<u8>, Error> {
        if let Some(algorithm) = self.compression_algorithm {
            for entry in &mut entries {
                entry.compress_in_place(algorithm);
            }
        }

        Ok(entries.to_zip())
    }
}

pub struct CompositeCache {
    pub compression_algorithm: Option<zpm_formats::CompressionAlgorithm>,

    pub global_cache: Option<DiskCache>,
    pub local_cache: Option<DiskCache>,
}

impl CompositeCache {
    pub fn new(compression_algorithm: Option<zpm_formats::CompressionAlgorithm>, global_cache: Option<DiskCache>, local_cache: Option<DiskCache>) -> Self {
        CompositeCache {
            compression_algorithm,
            global_cache,
            local_cache,
        }
    }

    pub fn packer(&self) -> CachePacker {
        CachePacker {
            compression_algorithm: self.compression_algorithm,
        }
    }

    pub fn key_path(&self, key: &Locator, ext: &str) -> Path {
        if let Some(ref cache) = self.local_cache {
            return cache.key_path(key, ext);
        }

        if let Some(ref cache) = self.global_cache {
            return cache.key_path(key, ext);
        }

        panic!("Expected at least one cache to be set");
    }

    pub fn cache_entry(&self, key: Locator, ext: &str) -> Result<InfoCacheEntry, Error> {
        if let Some(ref cache) = self.local_cache {
            return cache.cache_entry(key, ext);
        }

        if let Some(ref cache) = self.global_cache {
            return cache.cache_entry(key, ext);
        }

        panic!("Expected at least one cache to be set");
    }

    pub fn check_cache_entry(&self, key: Locator, ext: &str) -> Result<Option<InfoCacheEntry>, Error> {
        if let Some(ref cache) = self.local_cache {
            return cache.check_cache_entry(key, ext);
        }

        if let Some(ref cache) = self.global_cache {
            return cache.check_cache_entry(key, ext);
        }

        panic!("Expected at least one cache to be set");
    }

    pub fn has_cache_entry(&self, key: Locator, ext: &str) -> Result<bool, Error> {
        if let Some(ref cache) = self.local_cache {
            if cache.check_cache_entry(key.clone(), ext)?.is_some() {
                return Ok(true);
            }
        }

        if let Some(ref cache) = self.global_cache {
            if cache.check_cache_entry(key, ext)?.is_some() {
                return Ok(true);
            }
        }

        Ok(false)
    }

    async fn load<R, F>(key: Locator, ext: &str, func: F) -> Result<Vec<u8>, Error>
    where
        R: Future<Output = Result<Vec<u8>, Error>>,
        F: FnOnce() -> R,
    {
        crate::report::if_active_async(|report| {
            report.counters.fetch_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }).await;

        let res
            = func().await;

        if let Ok(data) = res.as_ref() {
            tracing::event!(
                target: "yarn::cache",
                tracing::Level::INFO,
                locator = %key.to_file_string(),
                extension = %ext,
                size_bytes = data.len(),
                "package downloaded",
            );

            crate::report::if_active_async(|report| {
                report.counters.fetch_size.fetch_add(data.len() as u32, std::sync::atomic::Ordering::Relaxed);
            }).await;
        }

        res
    }

    pub async fn ensure_blob<R, F>(&self, key: Locator, ext: &str, func: F) -> Result<CacheEntry, Error>
    where
        R: Future<Output = Result<Vec<u8>, Error>>,
        F: FnOnce() -> R,
    {
        if let Some(ref cache) = self.local_cache {
            return cache.ensure_blob(key.clone(), ext, || async {
                if let Some(ref cache) = self.global_cache {
                    Ok(cache.upsert_blob(key.clone(), ext, || Self::load(key, ext, func)).await?.data)
                } else {
                    Self::load(key, ext, func).await
                }
            }).await;
        }

        if let Some(ref cache) = self.global_cache {
            return cache.ensure_blob(key.clone(), ext, || Self::load(key, ext, func)).await;
        }

        panic!("Expected at least one cache to be set");
    }

    /// Force-refetch variant of [`ensure_blob`]: always invokes the
    /// fetcher and overwrites any existing entry.
    pub async fn refetch_blob<R, F>(&self, key: Locator, ext: &str, func: F) -> Result<CacheEntry, Error>
    where
        R: Future<Output = Result<Vec<u8>, Error>>,
        F: FnOnce() -> R,
    {
        if let Some(ref cache) = self.local_cache {
            return cache.refetch_blob(key.clone(), ext, || async {
                if let Some(ref cache) = self.global_cache {
                    Ok(cache.refetch_blob_data(key.clone(), ext, || Self::load(key, ext, func)).await?.data)
                } else {
                    Self::load(key, ext, func).await
                }
            }).await;
        }

        if let Some(ref cache) = self.global_cache {
            return cache.refetch_blob(key.clone(), ext, || Self::load(key, ext, func)).await;
        }

        panic!("Expected at least one cache to be set");
    }

    pub async fn upsert_blob<R, F>(&self, key: Locator, ext: &str, func: F) -> Result<DataCacheEntry, Error>
    where
        R: Future<Output = Result<Vec<u8>, Error>>,
        F: FnOnce() -> R,
    {
        if let Some(ref cache) = self.local_cache {
            return cache.upsert_blob(key.clone(), ext, || async {
                if let Some(ref cache) = self.global_cache {
                    Ok(cache.upsert_blob(key.clone(), ext, || Self::load(key, ext, func)).await?.data)
                } else {
                    Self::load(key, ext, func).await
                }
            }).await;
        }

        if let Some(ref cache) = self.global_cache {
            return cache.upsert_blob(key.clone(), ext, || Self::load(key, ext, func)).await;
        }

        panic!("Expected at least one cache to be set");
    }

    pub async fn refetch_blob_data<R, F>(&self, key: Locator, ext: &str, func: F) -> Result<DataCacheEntry, Error>
    where
        R: Future<Output = Result<Vec<u8>, Error>>,
        F: FnOnce() -> R,
    {
        if let Some(ref cache) = self.local_cache {
            return cache.refetch_blob_data(key.clone(), ext, || async {
                if let Some(ref cache) = self.global_cache {
                    Ok(cache.refetch_blob_data(key.clone(), ext, || Self::load(key, ext, func)).await?.data)
                } else {
                    Self::load(key, ext, func).await
                }
            }).await;
        }

        if let Some(ref cache) = self.global_cache {
            return cache.refetch_blob_data(key.clone(), ext, || Self::load(key, ext, func)).await;
        }

        panic!("Expected at least one cache to be set");
    }

    pub async fn clean(&self) -> Result<usize, Error> {
        if let Some(ref cache) = self.local_cache {
            return cache.clean().await;
        }

        Ok(0)
    }
}

pub struct DiskCache {
    cache_path: Path,
    name_suffix: String,
    immutable: bool,
    cleanable: bool,
    accessed_files: Arc<Mutex<HashSet<String>>>,
}

impl DiskCache {
    pub fn new(cache_path: Path, name_suffix: String, immutable: bool, cleanable: bool) -> Self {
        DiskCache {
            cache_path,
            name_suffix,
            immutable,
            cleanable,
            accessed_files: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    pub fn key_path(&self, locator: &Locator, ext: &str) -> Path {
        let key_name
            = format!("{}-{}{}{}", locator.slug(), CACHE_VERSION, self.name_suffix, ext);

        let key_path = self.cache_path
            .with_join_str(&key_name);

        if let Ok(mut accessed) = self.accessed_files.lock() {
            accessed.insert(key_name);
        }

        key_path
    }

    pub fn cache_entry(&self, key: Locator, ext: &str) -> Result<InfoCacheEntry, Error> {
        let key_path
            = self.key_path(&key, ext);

        Ok(InfoCacheEntry {
            path: key_path,
            checksum: None,
        })
    }

    pub fn check_cache_entry(&self, key: Locator, ext: &str) -> Result<Option<InfoCacheEntry>, Error> {
        let key_path
            = self.key_path(&key, ext);

        Ok(key_path.if_exists().map(|path| {
            InfoCacheEntry {
                path,
                checksum: None,
            }
        }))
    }

    pub async fn ensure_blob<R, F>(&self, key: Locator, ext: &str, func: F) -> Result<CacheEntry, Error>
    where
        R: Future<Output = Result<Vec<u8>, Error>>,
        F: FnOnce() -> R,
    {
        self.ensure_blob_inner(key, ext, func, false).await
    }

    /// Like [`ensure_blob`] but always invokes the fetcher, even on hit.
    pub async fn refetch_blob<R, F>(&self, key: Locator, ext: &str, func: F) -> Result<CacheEntry, Error>
    where
        R: Future<Output = Result<Vec<u8>, Error>>,
        F: FnOnce() -> R,
    {
        self.ensure_blob_inner(key, ext, func, true).await
    }

    async fn ensure_blob_inner<R, F>(&self, key: Locator, ext: &str, func: F, force_refetch: bool) -> Result<CacheEntry, Error>
    where
        R: Future<Output = Result<Vec<u8>, Error>>,
        F: FnOnce() -> R,
    {
        let key_path
            = self.key_path(&key, ext);
        let key_path_buf
            = key_path.to_path_buf();

        let exists
            = tokio::fs::try_exists(key_path_buf.clone()).await?;

        Ok(match exists {
            true if !force_refetch => {
                InfoCacheEntry {
                    path: key_path,
                    checksum: None,
                }.into()
            },

            true | false => {
                if self.immutable {
                    return Err(Error::ImmutableCache(key));
                }

                let data
                    = self.fetch_and_store_blob::<R, F>(key_path_buf, func).await?;

                tokio::task::spawn_blocking(move || {
                    let checksum
                        = Hash64::from_data(&data);

                    InfoCacheEntry {
                        path: key_path,
                        checksum: Some(checksum),
                    }.into()
                }).await.unwrap()
            },
        })
    }

    pub async fn upsert_blob<R, F>(&self, key: Locator, ext: &str, func: F) -> Result<DataCacheEntry, Error>
    where
        R: Future<Output = Result<Vec<u8>, Error>>,
        F: FnOnce() -> R,
    {
        self.upsert_blob_inner(key, ext, func, false).await
    }

    /// Force-refetch variant of [`upsert_blob`]: always invokes the
    /// fetcher and writes the bytes through.
    pub async fn refetch_blob_data<R, F>(&self, key: Locator, ext: &str, func: F) -> Result<DataCacheEntry, Error>
    where
        R: Future<Output = Result<Vec<u8>, Error>>,
        F: FnOnce() -> R,
    {
        self.upsert_blob_inner(key, ext, func, true).await
    }

    async fn upsert_blob_inner<R, F>(&self, key: Locator, ext: &str, func: F, force_refetch: bool) -> Result<DataCacheEntry, Error>
    where
        R: Future<Output = Result<Vec<u8>, Error>>,
        F: FnOnce() -> R,
    {
        let key_path
            = self.key_path(&key, ext);
        let key_path_buf
            = key_path.to_path_buf();

        if !force_refetch {
            match tokio::fs::read(key_path_buf.clone()).await {
                Ok(data) => {
                    return Ok(DataCacheEntry {
                        info: InfoCacheEntry {
                            path: key_path,
                            checksum: None,
                        },
                        data,
                    });
                },
                // Only NotFound counts as a cache miss; other IO errors
                // must surface so we don't blow past user-set permissions.
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {},
                Err(err) => return Err(err.into()),
            }
        }

        if self.immutable {
            return Err(Error::ImmutableCache(key));
        }

        let data
            = self.fetch_and_store_blob::<R, F>(key_path_buf, func).await?;

        Ok(tokio::task::spawn(async move {
            let checksum
                = Hash64::from_data(&data);

            DataCacheEntry {
                info: InfoCacheEntry {
                    path: key_path,
                    checksum: Some(checksum),
                },
                data,
            }
        }).await.unwrap())
    }

    async fn fetch_and_store_blob<R, F>(&self, key_path: PathBuf, func: F) -> Result<Vec<u8>, Error>
    where
        R: Future<Output = Result<Vec<u8>, Error>>,
        F: FnOnce() -> R,
    {
        let data
            = func().await?;

        let atomic_path
            = Path::try_from(key_path.clone())?;

        let write_result
            = atomic_path
                .fs_write_atomic(|tmp_path| -> Result<(), PathError> {
                    tmp_path.fs_write(&data)?;
                    Ok(())
                })
                .ok_exists();

        match write_result? {
            Some(_) => {
                Ok(data)
            },

            None => {
                Ok(tokio::fs::read(key_path).await?)
            },
        }
    }

    pub async fn clean(&self) -> Result<usize, Error> {
        if !self.cleanable {
            return Ok(0);
        }

        let accessed_files
            = self.accessed_files.lock()
                .map_err(|_| Error::Unsupported)?;

        let cache_entries = self.cache_path
            .fs_read_dir()?
            .collect::<Result<Vec<_>, _>>()?;

        let extraneous_cache_files = cache_entries
            .iter()
            .filter(|entry| entry.file_type().unwrap().is_file())
            .map(|entry| entry.file_name().to_os_string().into_string().unwrap())
            .filter(|file| !accessed_files.contains(file))
            .collect_vec();

        let extraneous_count
            = extraneous_cache_files.len();

        if extraneous_count > 0 && self.immutable {
            return Err(Error::ImmutableCacheCleanup(Path::try_from(extraneous_cache_files[0].clone()).unwrap()));
        }

        for file in extraneous_cache_files {
            self.cache_path
                .with_join_str(&file)
                .fs_rm_file()?;
        }

        Ok(extraneous_count)
    }
}
