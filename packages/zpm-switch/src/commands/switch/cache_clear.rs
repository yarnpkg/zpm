use clipanion::cli;
use zpm_utils::{IoResultExt, Path};

use crate::{cache, errors::Error};

#[cli::command]
#[cli::path("switch", "cache")]
#[cli::category("Cache management")]
#[cli::description("Clear all cached Yarn binaries")]
#[derive(Debug)]
pub struct CacheClearCommand {
    #[cli::option("-c,--clear,--clean")]
    _clear: bool,

    #[cli::option("--old", default = false)]
    #[cli::description("Clear cache entries older than 7 days")]
    old: bool,
}

impl CacheClearCommand {
    pub async fn execute(&self) -> Result<(), Error> {
        let cache_dir
            = cache::cache_dir()?;

        if self.old {
            let Some(cache_entries) = cache_dir.fs_read_dir().ok_missing()? else {
                return Ok(());
            };

            for entry in cache_entries {
                let entry
                    = entry?;

                let entry_path
                    = Path::try_from(entry.path())?;
                let entry_last_used
                    = cache::cache_last_used(&entry_path);

                let Ok(entry_last_used) = entry_last_used else {
                    continue;
                };

                if entry_last_used.elapsed().unwrap() > std::time::Duration::from_secs(60 * 60 * 24 * 7) {
                    entry_path.fs_rm()?;
                }
            }
        } else {
            cache_dir
                .fs_rm()
                .ok_missing()?;
        }

        Ok(())
    }
}
