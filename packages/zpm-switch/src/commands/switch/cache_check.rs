use clipanion::cli;
use zpm_utils::{get_system_string};

use crate::{cache, errors::Error};

/// Check if the specified versions are available in the cache
#[cli::command]
#[cli::path("switch", "cache")]
#[cli::category("Cache management")]
#[derive(Debug)]
pub struct CacheCheckCommand {
    #[cli::option("--check")]
    _check: bool,

    versions: Vec<zpm_semver::Version>,
}

impl CacheCheckCommand {
    pub async fn execute(&self) -> Result<(), Error> {
        for version in &self.versions {
            let cache_key = cache::CacheKey {
                cache_version: cache::CACHE_VERSION,
                version: version.clone(),
                platform: get_system_string().to_string(),
            };

            if !cache::check(&cache_key)? {
                return Err(Error::CacheNotFound(version.clone()));
            }
        }

        Ok(())
    }
}
