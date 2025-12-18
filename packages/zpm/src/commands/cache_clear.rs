use clipanion::cli;
use itertools::Itertools;
use zpm_utils::{DataType, IoResultExt, Path};

use crate::{error::Error, project, report::{StreamReport, StreamReportConfig, current_report, with_report_result}};

/// Clear the global cache
#[cli::command]
#[cli::path("cache", "clear")]
#[cli::path("cache", "clean")]
#[cli::category("Cache management")]
pub struct CacheClear {
    /// Clear cache entries older than 7 days
    #[cli::option("--old", default = false)]
    old: bool,
}

impl CacheClear {
    pub async fn execute(&self) -> Result<(), Error> {
        clear_cache(self.old).await
    }
}

#[cli::command]
#[cli::path("cache")]
#[cli::category("Cache management")]
pub struct CacheClear2 {
    #[cli::option("-c,--clear,--clean")]
    _clear: bool,

    /// Clear cache entries older than 7 days
    #[cli::option("--old", default = false)]
    old: bool,
}

impl CacheClear2 {
    pub async fn execute(&self) -> Result<(), Error> {
        clear_cache(self.old).await
    }
}

async fn clear_cache(old: bool) -> Result<(), Error> {
    let project
        = project::Project::new(None).await?;

    let report = StreamReport::new(StreamReportConfig {
        ..StreamReportConfig::from_config(&project.config)
    });

    with_report_result(report, async {
        let cache_entries
            = project.global_cache_path()
                .fs_read_dir()
                .ok_missing()?;

        let mut cleared_entries
            = 0;

        if let Some(cache_entries) = cache_entries {
            let cache_entries = cache_entries
                .filter_map(|entry| entry.ok())
                .map(|entry| Path::try_from(entry.path()))
                .filter_map(|entry| entry.ok())
                .filter(|entry| !old || age_filter(entry))
                .collect_vec();

            cleared_entries
                = cache_entries.len();

            for entry in &cache_entries {
                entry.fs_rm().ok_missing()?;
            }
        }

        current_report().await.as_mut().map(|report| {
            if cleared_entries > 0 {
                report.info(format!("Cleared {} entries from the cache.", DataType::Number.colorize(&cleared_entries.to_string())))
            } else {
                report.info("No entries to clear from the cache.".to_string());
            }
        });

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
