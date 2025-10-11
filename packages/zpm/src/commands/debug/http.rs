use clipanion::cli;

use crate::{error::Error, project::Project, report::{with_report_result, StreamReport, StreamReportConfig}};

#[cli::command]
#[cli::path("debug", "http")]
pub struct Http {
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
