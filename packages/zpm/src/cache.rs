use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::Arc;
use arca::Path;
use bincode::{self, Decode, Encode};
use futures::Future;
use serde::{Deserialize, Serialize};
use sha2::Digest;
use tokio::io::AsyncReadExt;

use crate::error::Error;
use crate::hash::Sha256;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
pub struct CachedBlob {
    pub path: Path,
    pub data: Vec<u8>,
    pub checksum: Option<Sha256>,
}

#[derive(Debug)]
pub struct SerializedResult<T> {
    pub path: Path,
    pub data: T,
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

    pub async fn upsert_blob<K, R, F>(&self, key: K, ext: &str, func: F) -> Result<CachedBlob, Error>
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

    pub async fn upsert_serialized<K, T, R, F>(&self, key: K, func: F) -> Result<SerializedResult<T>, Error>
    where
        K: Clone + Encode + Decode,
        T: Encode + Decode + std::fmt::Debug,
        R: Future<Output = Result<T, Error>>,
        F: FnOnce() -> R,
    {
        if let Some(ref cache) = self.local_cache {
            return cache.upsert_serialized(key.clone(), || async {
                if let Some(ref cache) = self.global_cache {
                    Ok(cache.upsert_serialized(key, func).await?.data)
                } else {
                    func().await
                }
            }).await;
        }

        if let Some(ref cache) = self.global_cache {
            return cache.upsert_serialized(key, func).await;
        }

        panic!("Cache miss");
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
        let serialized_key = bincode::encode_to_vec(&key, self.data_config)
            .map_err(Arc::new)?;

        let mut key = sha2::Sha256::new();
        key.update(serialized_key);
        let key = key.finalize();

        let key_path = self.cache_path
            .with_join_str(format!("{:064x}{}", key, ext));

        Ok(key_path)
    }

    pub async fn upsert_blob<K, R, F>(&self, key: K, ext: &str, func: F) -> Result<CachedBlob, Error>
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
                CachedBlob {
                    path: key_path,
                    data,
                    checksum: None,
                }    
            },

            Err(err) => {
                if err.kind() != std::io::ErrorKind::NotFound {
                    return Err(err)?;
                }

                let data = self.fetch_and_store_blob::<R, F>(key_path_buf, func).await?;

                tokio::task::spawn(async move {
                    let checksum
                        = Some(Sha256::from_data(&data));

                    CachedBlob {
                        path: key_path,
                        data,
                        checksum,
                    }
                }).await.unwrap()
            },
        })
    }

    pub async fn upsert_serialized<K, T, R, F>(&self, key: K, func: F) -> Result<SerializedResult<T>, Error>
    where
        K: Encode + Decode,
        T: Encode + Decode + std::fmt::Debug,
        R: Future<Output = Result<T, Error>>,
        F: FnOnce() -> R,
    {
        let serialized_key = bincode::encode_to_vec(&key, self.data_config)
            .map_err(Arc::new)?;
    
        let mut key = sha2::Sha256::new();
        key.update(serialized_key);
        let key = key.finalize();

        let key_path = self.cache_path
            .with_join_str(format!("{:064x}.dat", key));

        let key_path_buf = key_path
            .to_path_buf();

        let data = match tokio::fs::File::open(&key_path_buf).await {
            Ok(mut file) => {
                let mut buffer = Vec::new();

                file.read_to_end(&mut buffer).await?;

                let decode: Result<(T, _), _>
                    = bincode::decode_from_slice(&buffer, self.data_config);

                match decode {
                    Ok((data, _)) => {
                        Ok(data)
                    }

                    Err(_) => {
                        self.fetch_and_store_serialized::<T, R, F>(key_path_buf, func).await
                    }
                }
            }

            Err(_) => {
                self.fetch_and_store_serialized::<T, R, F>(key_path_buf, func).await
            }
        };

        data.map(|data| SerializedResult {
            path: key_path,
            data,
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

    async fn fetch_and_store_serialized<T, R, F>(&self, key_path: PathBuf, func: F) -> Result<T, Error>
    where
        T: Encode + Decode + std::fmt::Debug,
        R: Future<Output = Result<T, Error>>,
        F: FnOnce() -> R,
    {
        let data = func().await?;

        let encoded = bincode::encode_to_vec(&data, self.data_config)?;
        std::fs::write(key_path, encoded)?;

        Ok(data)
    }
}
