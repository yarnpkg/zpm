use clipanion::cli;
use itertools::Itertools;
use zpm_utils::{DataType, IoResultExt, Path};

use crate::{error::Error, project, report::{StreamReport, StreamReportConfig, with_report_result}};

/// Remove cache files
///
/// By default this command removes only the per-project cache. The mirror, which is the project-global registry cache, is left untouched unless
/// `--mirror` or `--all` is set.
#[cli::command]
#[cli::path("cache", "clear")]
#[cli::path("cache", "clean")]
#[cli::category("Cache management")]
pub struct CacheClear {
    /// Remove only cache entries older than seven days
    #[cli::option("--old", default = false)]
    old: bool,

    /// Remove the registry mirror cache instead of the project cache
    #[cli::option("--mirror", default = false)]
    mirror: bool,

    /// Remove both the project cache and the registry mirror
    #[cli::option("--all", default = false)]
    all: bool,
}

impl CacheClear {
    pub async fn execute(&self) -> Result<(), Error> {
        clear_cache(self.old, self.mirror, self.all).await
    }
}

/// Remove cache files
///
/// This is a shorthand for `yarn cache clear` when used with `--clear` or `--clean`.
///
#[cli::command]
#[cli::path("cache")]
#[cli::category("Cache management")]
pub struct CacheClear2 {
    /// Run the cache cleanup operation
    #[cli::option("-c,--clear,--clean")]
    _clear: bool,

    /// Remove only cache entries older than seven days
    #[cli::option("--old", default = false)]
    old: bool,

    /// Remove the registry mirror cache instead of the project cache
    #[cli::option("--mirror", default = false)]
    mirror: bool,

    /// Remove both the project cache and the registry mirror
    #[cli::option("--all", default = false)]
    all: bool,
}

impl CacheClear2 {
    pub async fn execute(&self) -> Result<(), Error> {
        clear_cache(self.old, self.mirror, self.all).await
    }
}

async fn clear_cache(old: bool, mirror: bool, all: bool) -> Result<(), Error> {
    let project
        = project::Project::new(None).await?;

    if !project.config.settings.enable_cache_clean.value {
        return Err(Error::CacheCleanDisabled);
    }

    let report = StreamReport::new(StreamReportConfig {
        ..StreamReportConfig::from_config(&project.config)
    });

    let local_cache_path = project.local_cache_path();
    let global_cache_path = project.global_cache_path();

    let (clean_local, clean_mirror) = if all {
        (true, true)
    } else if mirror {
        (false, true)
    } else {
        (true, false)
    };

    with_report_result(report, async move {
        let mut cleared_entries
            = 0;

        for (do_clean, cache_path) in [(clean_local, &local_cache_path), (clean_mirror, &global_cache_path)] {
            if !do_clean {
                continue;
            }

            let entries = cache_path
                .fs_read_dir()
                .ok_missing()?;

            if let Some(entries) = entries {
                let entries = entries
                    .filter_map(|entry| entry.ok())
                    .map(|entry| Path::try_from(entry.path()))
                    .filter_map(|entry| entry.ok())
                    .filter(|entry| !old || age_filter(entry))
                    .collect_vec();

                cleared_entries += entries.len();

                for entry in &entries {
                    entry.fs_rm().ok_missing()?;
                }

                if !old && cache_path.fs_exists() {
                    cache_path.fs_rm().ok_missing()?;
                }
            }
        }

        crate::report::if_active_async(|report| {
            if cleared_entries > 0 {
                report.info(format!("Cleared {} entries from the cache.", DataType::Number.colorize(&cleared_entries.to_string())));
            } else {
                report.info("No entries to clear from the cache.".to_string());
            }
        }).await;

        Ok(())
    }).await?;

    Ok(())
}

fn age_filter(entry: &Path) -> bool {
    let entry_last_used
        = entry.fs_metadata().ok()
            .and_then(|metadata| metadata.modified().ok());

    let Some(entry_last_used) = entry_last_used else {
        return false;
    };

    let Ok(elapsed) = entry_last_used.elapsed() else {
        return false;
    };

    elapsed > std::time::Duration::from_secs(60 * 60 * 24 * 7)
}
