use clipanion::cli;
use zpm_utils::DataType;

use crate::{
    error::Error, http_npm::get_registry, project::Project, report::{current_report, with_report_result, StreamReport, StreamReportConfig}
};

#[cli::command]
#[cli::path("npm", "logout")]
#[cli::category("Npm-related commands")]
#[cli::description("Logout from the npm registry")]
pub struct Logout {
    #[cli::option("-s,--scope")]
    #[cli::description("Get the token for a given scope")]
    scope: Option<String>,

    #[cli::option("--publish", default = false)]
    #[cli::description("Login to the publish registry")]
    publish: bool,
}

impl Logout {
    #[tokio::main]
    pub async fn execute(&self) -> Result<(), Error> {
        let project
            = Project::new(None).await?;

        let report = StreamReport::new(StreamReportConfig {
            ..StreamReportConfig::from_config(&project.config)
        });

        with_report_result(report, async {
            let registry
                = get_registry(&project.config, self.scope.as_deref(), self.publish)?
                    .to_string();

            let Some(config_path) = project.config.user_config_path else {
                return Err(Error::AuthenticationError("Failed to get user config path".to_string()));
            };

            let config_content = config_path
                .fs_read_text()?;

            let updated_content = zpm_parsers::yaml::Yaml::update_document_field(
                &config_content,
                zpm_parsers::Path::from_segments(vec![
                    "npmRegistries".to_string(),
                    registry.to_string(),
                    "npmAuthToken".to_string(),
                ]),
                zpm_parsers::Value::Undefined,
            )?;

            config_path
                .fs_write_text(&updated_content)?;

            current_report().await.as_mut().map(|report| {
                report.info(format!("Successfully logged out from {}", DataType::Url.colorize(&registry)));
            });

            Ok(())
        }).await
    }
}
