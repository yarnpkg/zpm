use clipanion::cli;

use crate::{error::Error, project::Project, report::{with_report_result, StreamReport, StreamReportConfig}};

/// Fetch a URL through Yarn's HTTP client
///
/// This debug command performs a GET request after preparing the project environment, which makes it useful for testing registry and network
/// configuration.
///
#[cli::command]
#[cli::path("debug", "http")]
pub struct Http {
    /// URL to request
    url: String,
}

impl Http {
    pub async fn execute(&self) -> Result<(), Error> {
        let mut project
            = Project::new(None).await?;

        project
            .lazy_install().await?;

        let report = StreamReport::new(StreamReportConfig {
            ..StreamReportConfig::default()
        });

        with_report_result(report, async {
            project.http_client
                .get(&self.url)?
                .send()
                .await?
                .text()
                .await?;

            Ok(())
        }).await?;

        Ok(())
    }
}
