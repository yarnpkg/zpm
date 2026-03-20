use clipanion::cli;
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
        let cache_entries = match project.global_cache_path()
            .fs_read_dir()
            .await
            .ok_missing()?
        {
            Some(cache_entries) => cache_entries,
            None => {
                current_report().await.as_ref().map(|report| {
                    report.info("No entries to clear from the cache.".to_string());
                });
                return Ok(());
            },
        };

        let mut entries_to_delete = Vec::new();
        let mut cache_entries = cache_entries;
        while let Some(entry) = cache_entries.next_entry().await? {
            let entry = Path::try_from(entry.path())?;

            if !old || age_filter(&entry) {
                entries_to_delete.push(entry);
            }
        }

        let cleared_entries = entries_to_delete.len();

        for entry in &entries_to_delete {
            entry.fs_rm().await.ok_missing()?;
        }

        current_report().await.as_ref().map(|report| {
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
