use clipanion::cli;
use zpm_parsers::{ops::Ops, yaml::Yaml, Parser};

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

            let fields
                = Yaml::parse(&config_content)?;

            let mut ops
                = Ops::new();

            for field in &fields {
                if field.path.len() == 3 && field.path[0] == "npmRegistries" && field.path[2] == "npmAuthToken" {
                    ops.set(field.path.clone(), zpm_parsers::Value::Undefined);
                }
            }

            let updated_content
                = ops.derive::<Yaml>(&fields)
                    .apply_to_document(&config_content);

            config_path
                .fs_write_text(&updated_content)?;

            current_report().await.as_ref().map(|report| {
                report.info("Successfully logged out from all npm registries".to_string());
            });

            Ok(())
        }).await
    }
}
