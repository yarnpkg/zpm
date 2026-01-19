use clipanion::cli;
use zpm_parsers::{Document, DataDocument, Value};

use crate::{
    error::Error,
    project::Project, report::{current_report, with_report_result, StreamReport, StreamReportConfig},
};

/// Logout from all npm registries
#[cli::command]
#[cli::path("npm", "logout")]
#[cli::category("Npm-related commands")]
pub struct LogoutAll {
    #[cli::option("-A,--all")]
    _all: bool,
}

impl LogoutAll {
    pub async fn execute(&self) -> Result<(), Error> {
        let project
            = Project::new(None).await?;

        let report = StreamReport::new(StreamReportConfig {
            ..StreamReportConfig::from_config(&project.config)
        });

        with_report_result(report, async {
            let Some(config_path) = project.config.user_config_path else {
                return Err(Error::AuthenticationError("Failed to get user config path".to_string()));
            };

            let config_content = config_path
                .fs_read_text()?;

            let mut doc
                = DataDocument::new(config_content.into_bytes())?;

            let paths_to_remove: Vec<_>
                = doc.paths().keys()
                    .filter(|path| {
                        path.len() == 3
                            && path[0] == "npmRegistries"
                            && path[2] == "npmAuthToken"
                    })
                    .cloned()
                    .collect();

            for path in paths_to_remove {
                doc.set_path(&path, Value::Undefined)?;
            }

            let updated_content
                = String::from_utf8(doc.input().to_vec())
                    .expect("Document was originally valid UTF-8");

            config_path
                .fs_write_text(&updated_content)?;

            current_report().await.as_ref().map(|report| {
                report.info("Successfully logged out from all npm registries".to_string());
            });

            Ok(())
        }).await
    }
}
