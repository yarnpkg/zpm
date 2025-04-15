use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use std::collections::HashSet;
use std::sync::Mutex;
use zpm_utils::Path;
use bincode;
use futures::Future;
use sha2::Digest;
use zpm_macros::parse_enum;
use zpm_utils::ToHumanString;

use crate::error::Error;
use crate::hash::Sha256;
use crate::primitives::locator::Locator;

#[parse_enum]
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[derive_variants(Clone, Debug, PartialEq, Eq, Hash)]
pub enum CacheEntry {
    #[no_pattern]
    Info {
        path: Path,
        checksum: Option<Sha256>,
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

pub struct CompositeCache {
    pub global_cache: Option<DiskCache>,
    pub local_cache: Option<DiskCache>,
}

impl CompositeCache {
    pub fn key_path(&self, key: &Locator, ext: &str) -> Result<Path, Error> {
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

    pub async fn ensure_blob<R, F>(&self, key: Locator, ext: &str, func: F) -> Result<CacheEntry, Error>
    where
        R: Future<Output = Result<Vec<u8>, Error>>,
        F: FnOnce() -> R,
    {
        if let Some(ref cache) = self.local_cache {
            return cache.ensure_blob(key.clone(), ext, || async {
                if let Some(ref cache) = self.global_cache {
                    Ok(cache.upsert_blob(key, ext, func).await?.data)
                } else {
                    func().await
                }
            }).await;
        }

        if let Some(ref cache) = self.global_cache {
            return cache.ensure_blob(key, ext, func).await;
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
                    Ok(cache.upsert_blob(key, ext, func).await?.data)
                } else {
                    func().await
                }
            }).await;
        }

        if let Some(ref cache) = self.global_cache {
            return cache.upsert_blob(key, ext, func).await;
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
    data_config: bincode::config::Configuration,
    immutable: bool,
    accessed_files: Arc<Mutex<HashSet<String>>>,
}

impl DiskCache {
    pub fn new(cache_path: Path, immutable: bool) -> Self {
        DiskCache {
            cache_path,
            data_config: bincode::config::standard(),
            immutable,
            accessed_files: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    pub fn key_path(&self, key: &Locator, ext: &str) -> Result<Path, Error> {
        let serialized_key = bincode::encode_to_vec(key, self.data_config)
            .map_err(Arc::new)?;

        let mut key = sha2::Sha256::new();
        key.update(serialized_key);
        let key = key.finalize();

        let key_name
            = format!("{:064x}{}", key, ext);

        let key_path = self.cache_path
            .with_join_str(&key_name);

        if let Ok(mut accessed) = self.accessed_files.lock() {
            accessed.insert(key_name);
        }

        Ok(key_path)
    }

    pub fn cache_entry(&self, key: Locator, ext: &str) -> Result<InfoCacheEntry, Error> {
        let key_path = self.key_path(&key, ext)?;

        Ok(InfoCacheEntry {
            path: key_path,
            checksum: None,
        })
    }

    pub fn check_cache_entry(&self, key: Locator, ext: &str) -> Result<Option<InfoCacheEntry>, Error> {
        let key_path = self.key_path(&key, ext)?;

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
        let key_path = self.key_path(&key, ext)?;
        let key_path_buf = key_path.to_path_buf();

        let exists = tokio::fs::try_exists(key_path_buf.clone()).await?;

        Ok(match exists {
            true => {
                InfoCacheEntry {
                    path: key_path,
                    checksum: None,
                }.into()
            },

            false => {
                if self.immutable {
                    return Err(Error::ImmutableCache(key.to_print_string()));
                }

                let data = self.fetch_and_store_blob::<R, F>(key_path_buf, func).await?;

                tokio::task::spawn_blocking(move || {
                    let checksum = Sha256::from_data(&data);

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
        let key_path = self.key_path(&key, ext)?;
        let key_path_buf = key_path.to_path_buf();

        let read = tokio::fs::read(key_path_buf.clone()).await;

        Ok(match read {
            Ok(data) => {
                DataCacheEntry {
                    info: InfoCacheEntry {
                        path: key_path,
                        checksum: None,
                    },
                    data,
                }    
            },

            Err(err) => {
                if err.kind() != std::io::ErrorKind::NotFound {
                    return Err(err)?;
                }

                if self.immutable {
                    return Err(Error::ImmutableCache(key.to_print_string()));
                }

                let data = self.fetch_and_store_blob::<R, F>(key_path_buf, func).await?;

                tokio::task::spawn(async move {
                    let checksum = Sha256::from_data(&data);

                    DataCacheEntry {
                        info: InfoCacheEntry {
                            path: key_path,
                            checksum: Some(checksum),
                        },
                        data,
                    }
                }).await.unwrap()
            },
        })
    }

    async fn fetch_and_store_blob<R, F>(&self, key_path: PathBuf, func: F) -> Result<Vec<u8>, Error>
    where
        R: Future<Output = Result<Vec<u8>, Error>>,
        F: FnOnce() -> R,
    {
        let data = func().await?;

        let mut file = File::create(key_path.clone())?;
        file.write_all(&data)?;

        Ok(data)
    }

    pub async fn clean(&self) -> Result<usize, Error> {
        let accessed_files = match self.accessed_files.lock() {
            Ok(accessed) => accessed.clone(),
            Err(_) => return Err(Error::Unsupported),
        };

        let cache_entries = self.cache_path
            .fs_read_dir()?
            .collect::<Result<Vec<_>, _>>()?;

        let extraneous_cache_files = cache_entries
            .iter()
            .filter(|entry| entry.file_type().unwrap().is_file())
            .map(|entry| entry.file_name().to_os_string().into_string().unwrap())
            .filter(|file| !accessed_files.contains(file))
            .collect::<Vec<_>>();

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
