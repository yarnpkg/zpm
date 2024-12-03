use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use arca::Path;
use bincode::{self, Decode, Encode};
use futures::Future;
use sha2::Digest;
use zpm_macros::parse_enum;

use crate::error::Error;
use crate::hash::Sha256;

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

#[derive(Clone)]
pub struct CompositeCache {
    pub global_cache: Option<DiskCache>,
    pub local_cache: Option<DiskCache>,
}

impl CompositeCache {
    pub fn key_path<K: Decode + Encode>(&self, key: &K, ext: &str) -> Result<Path, Error> {
        if let Some(ref cache) = self.local_cache {
            return cache.key_path(key, ext);
        }

        if let Some(ref cache) = self.global_cache {
            return cache.key_path(key, ext);
        }

        panic!("Expected at least one cache to be set");
    }

    pub async fn ensure_blob<K, R, F>(&self, key: K, ext: &str, func: F) -> Result<CacheEntry, Error>
    where
        K: Clone + Decode + Encode,
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

    pub async fn upsert_blob<K, R, F>(&self, key: K, ext: &str, func: F) -> Result<DataCacheEntry, Error>
    where
        K: Clone + Decode + Encode,
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
}

#[derive(Clone)]
pub struct DiskCache {
    cache_path: Path,
    data_config: bincode::config::Configuration,
}

impl DiskCache {
    pub fn new(cache_path: Path) -> Self {
        fs::create_dir_all(cache_path.to_path_buf())
            .unwrap();

        DiskCache {
            cache_path,
            data_config: bincode::config::standard(),
        }
    }

    pub fn key_path<K: Decode + Encode>(&self, key: &K, ext: &str) -> Result<Path, Error> {
        let serialized_key = bincode::encode_to_vec(key, self.data_config)
            .map_err(Arc::new)?;

        let mut key = sha2::Sha256::new();
        key.update(serialized_key);
        let key = key.finalize();

        let key_path = self.cache_path
            .with_join_str(format!("{:064x}{}", key, ext));

        Ok(key_path)
    }

    pub async fn ensure_blob<K, R, F>(&self, key: K, ext: &str, func: F) -> Result<CacheEntry, Error>
    where
        K: Decode + Encode,
        R: Future<Output = Result<Vec<u8>, Error>>,
        F: FnOnce() -> R,
    {
        let key_path
            = self.key_path(&key, ext)?;
        let key_path_buf
            = key_path.to_path_buf();

        let exists
            = tokio::fs::try_exists(key_path_buf.clone()).await?;

        Ok(match exists {
            true => {
                InfoCacheEntry {
                    path: key_path,
                    checksum: None,
                }.into()
            },

            false => {
                let data = self.fetch_and_store_blob::<R, F>(key_path_buf, func).await?;

                tokio::task::spawn(async move {
                    let checksum
                        = Sha256::from_data(&data);

                    InfoCacheEntry {
                        path: key_path,
                        checksum: Some(checksum),
                    }.into()
                }).await.unwrap()
            },
        })
    }

    pub async fn upsert_blob<K, R, F>(&self, key: K, ext: &str, func: F) -> Result<DataCacheEntry, Error>
    where
        K: Decode + Encode,
        R: Future<Output = Result<Vec<u8>, Error>>,
        F: FnOnce() -> R,
    {
        let key_path
            = self.key_path(&key, ext)?;
        let key_path_buf
            = key_path.to_path_buf();

        let read
            = tokio::fs::read(key_path_buf.clone()).await;

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

                let data = self.fetch_and_store_blob::<R, F>(key_path_buf, func).await?;

                tokio::task::spawn(async move {
                    let checksum
                        = Sha256::from_data(&data);

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
}
