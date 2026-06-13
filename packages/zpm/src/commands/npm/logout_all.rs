use clipanion::cli;
use zpm_parsers::{Document, DataDocument, Value};

use crate::{
    error::Error,
    project::Project, report::{with_report_result, StreamReport, StreamReportConfig},
};

/// Remove all stored npm registry credentials
#[cli::command]
#[cli::path("npm", "logout")]
#[cli::category("Npm-related commands")]
pub struct LogoutAll {
    /// Remove credentials for every configured npm registry and scope
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
                            && (path[0] == "npmRegistries" || path[0] == "npmScopes")
                            && (path[2] == "npmAuthToken" || path[2] == "npmAuthIdent")
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

            crate::report::if_active_async(|report| {
                report.info("Successfully logged out from all npm registries".to_string());
            }).await;

            Ok(())
        }).await
    }
}
