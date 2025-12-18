use clipanion::cli;
use zpm_utils::DataType;

use crate::{
    error::Error, http_npm::get_registry, project::Project, report::{current_report, with_report_result, StreamReport, StreamReportConfig}
};

/// Logout from the npm registry
///
/// This command will log you out by modifying your local configuration (in your home folder, never in the project itself) to delete all credentials linked to a registry.
///
/// Adding the `-s,--scope` flag will cause the deletion to be done against whatever registry is configured for the associated scope (see also `npmScopes`).
///
/// Adding the `--publish` flag will cause the deletion to be done against the registry used when publishing the package (see also `publishConfig.registry` and `npmPublishRegistry`).
///
/// Adding the `-A,--all` flag will cause the deletion to be done against all registries and scopes.
///
#[cli::command]
#[cli::path("npm", "logout")]
#[cli::category("Npm-related commands")]
pub struct Logout {
    /// Logout from the registry configured for a given scope
    #[cli::option("-s,--scope")]
    scope: Option<String>,

    /// Logout from the publish registry
    #[cli::option("--publish", default = false)]
    publish: bool,
}

impl Logout {
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

            current_report().await.as_ref().map(|report| {
                report.info(format!("Successfully logged out from {}", DataType::Url.colorize(&registry)));
            });

            Ok(())
        }).await
    }
}
