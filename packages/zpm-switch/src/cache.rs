use std::{future::Future, io::Write};

use serde::{Deserialize, Serialize};
use zpm_utils::{DataType, Hash64, IoResultExt, Path, ToFileString, ToHumanString, Unit};

use crate::errors::Error;

pub const CACHE_VERSION: usize = 1;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheKey {
    pub cache_version: usize,
    pub version: zpm_semver::Version,
    pub platform: String,
}

impl CacheKey {
    pub fn to_url(&self) -> String {
        format!("https://repo.yarnpkg.com/releases/{}/{}", self.version.to_file_string(), self.platform)
    }
}

pub fn cache_dir() -> Result<Path, Error> {
    let cache_dir = Path::home_dir()?
        .ok_or(Error::MissingHomeFolder)?
        .with_join_str(".yarn/switch/cache");

    Ok(cache_dir)
}

pub fn cache_metadata(p: &Path) -> Result<CacheKey, Error> {
    let key_string = p
        .with_join_str("meta.json")
        .fs_read_text()?;

    let key_data: CacheKey
        = sonic_rs::from_str(&key_string)?;

    Ok(key_data)
}

pub fn cache_last_used(p: &Path) -> Result<std::time::SystemTime, Error> {
    let ready_path = p
        .with_join_str(".ready");

    let metadata
        = ready_path.fs_metadata()?;

    Ok(metadata.modified()?)
}

async fn pretty_download<F: Future<Output = Result<(), Error>>>(key_data: &CacheKey, f: F) -> Result<(), Error> {
    if zpm_ci::is_terminal() {
        print!(
            "{} · Downloading Yarn {} …",
            DataType::Info.colorize("➤"),
            key_data.version.to_print_string(),
        );

        std::io::stdout()
            .flush()
            .unwrap();
    }

    let start_time
        = std::time::Instant::now();

    let result
        = f.await;

    let duration
        = std::time::Instant::now() - start_time;

    if zpm_ci::is_terminal() {
        if result.is_ok() {
            println!(
                "\x1b[2K\r{} · Downloaded Yarn {} in {}.",
                DataType::Success.colorize("✓"),
                key_data.version.to_print_string(),
                Unit::duration(duration.as_secs_f64()).to_print_string(),
            );
        } else {
            println!(
                "\x1b[2K\r{} · Failed to download Yarn {} after {}.",
                DataType::Error.colorize("✗"),
                key_data.version.to_print_string(),
                Unit::duration(duration.as_secs_f64()).to_print_string(),
            );
        }

        println!();
    }

    result
}

fn access(key_data: &CacheKey) -> Result<(Path, bool), Error> {
    let key_string
        = sonic_rs::to_string(key_data).unwrap();
    let key_hash
        = Hash64::from_string(&key_string);

    let cache_path = Path::home_dir()?
        .ok_or(Error::MissingHomeFolder)?
        .with_join(&cache_dir()?)
        .with_join_str(key_hash.short());

    let ready_path = cache_path
        .with_join_str(".ready");

    Ok((cache_path, ready_path.fs_exists()))
}

pub fn check(key_data: &CacheKey) -> Result<bool, Error> {
    Ok(access(key_data)?.1)
}

pub async fn ensure<R: Future<Output = Result<(), Error>>, F: FnOnce(Path) -> R>(key_data: &CacheKey, f: F) -> Result<Path, Error> {
    match access(key_data)? {
        (cache_path, true) => {
            let ready_path = cache_path
                .with_join_str(".ready");

            // Not a big deal if this fails, which may happen on filesystems
            // with limited permissions (read-only ones)
            let _ = ready_path
                .fs_set_modified(std::time::SystemTime::now());

            Ok(cache_path)
        },

        (cache_path, false) => {
            pretty_download(key_data, async {
                let temp_dir
                    = Path::temp_dir()?;

                f(temp_dir.clone()).await?;

                temp_dir
                    .with_join_str("meta.json")
                    .fs_write(sonic_rs::to_string(&key_data)?)?;

                temp_dir
                    .with_join_str(".ready")
                    .fs_write([])?;

                cache_path
                    .fs_create_parent()?;

                temp_dir
                    .fs_concurrent_move(&cache_path)?;

                Ok(())
            }).await?;

            Ok(cache_path)
        },
    }
}
