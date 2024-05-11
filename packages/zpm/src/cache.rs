use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::Arc;
use arca::Path;
use bincode::{self, Decode, Encode};
use futures::Future;
use once_cell::sync::Lazy;
use sha2::Digest;

use crate::error::Error;
use crate::hash::Sha256;
use crate::project;

pub static PACKAGE_CACHE: Lazy<DiskCache> = Lazy::new(|| {
    DiskCache::new(project::root().unwrap().with_join_str("node_modules/.zpm"))
});

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

    pub async fn upsert_blob<K, R, F>(&self, key: K, ext: &str, func: F) -> Result<(Path, Vec<u8>, Sha256), Error>
    where
        K: Decode + Encode,
        R: Future<Output = Result<Vec<u8>, Error>>,
        F: FnOnce() -> R,
    {
        let serialized_key = bincode::encode_to_vec(&key, self.data_config)
            .map_err(Arc::new)?;

        let mut key = sha2::Sha256::new();
        key.update(serialized_key);
        let key = key.finalize();

        let key_path = self.cache_path
            .with_join_str(format!("{:064x}{}", key, ext));

        let key_path_buf = key_path
            .to_path_buf();

        let read
            = tokio::fs::read(key_path_buf.clone()).await;

        let data = match read {
            Ok(data) => data,
            Err(err) => {
                if err.kind() != std::io::ErrorKind::NotFound {
                    return Err(Error::IoError(Arc::new(err)));
                }
        
                self.fetch_and_store_blob::<R, F>(key_path_buf, func).await?
            },
        };

        Ok(tokio::task::spawn(async move {
            let checksum = Sha256::from_data(&data);
            (key_path, data, checksum)
        }).await.unwrap())
    }

    pub async fn upsert_serialized<K, T, R, F>(&self, key: K, func: F) -> Result<(Path, T), Error>
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

        let data = match File::open(&key_path_buf) {
            Ok(mut file) => {
                let mut buffer = Vec::new();

                file.read_to_end(&mut buffer)
                    .map_err(Arc::new)?;

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

        data.map(|data| (key_path, data))
    }

    async fn fetch_and_store_blob<R, F>(&self, key_path: PathBuf, func: F) -> Result<Vec<u8>, Error>
    where
        R: Future<Output = Result<Vec<u8>, Error>>,
        F: FnOnce() -> R,
    {
        let data = func().await?;

        let mut file = File::create(key_path.clone())
            .map_err(Arc::new)?;

        file.write_all(&data)
            .map_err(Arc::new)?;

        Ok(data)
    }

    async fn fetch_and_store_serialized<T, R, F>(&self, key_path: PathBuf, func: F) -> Result<T, Error>
    where
        T: Encode + Decode + std::fmt::Debug,
        R: Future<Output = Result<T, Error>>,
        F: FnOnce() -> R,
    {
        let data = func().await?;

        let mut file = File::create(key_path.clone())
            .map_err(Arc::new)?;

        let encoded = bincode::encode_to_vec(&data, self.data_config)
            .map_err(Arc::new)?;

        file.write_all(&encoded)
            .map_err(Arc::new)?;

        Ok(data)
    }
}
