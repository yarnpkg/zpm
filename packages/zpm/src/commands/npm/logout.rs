use clipanion::cli;
use zpm_parsers::DataDocument;
use zpm_utils::DataType;

use crate::{
    error::Error, http_npm::get_registry, project::Project, report::{with_report_result, StreamReport, StreamReportConfig}
};

/// Remove credentials for an npm registry
///
/// This command edits the home configuration file to remove credentials linked to a registry. It never edits project configuration.
///
/// Use `-s,--scope` to remove credentials stored for a scope.
///
/// Use `--publish` to remove credentials for the publish registry.
///
/// Use `-A,--all` to remove credentials for all registries and scopes.
///
#[cli::command]
#[cli::path("npm", "logout")]
#[cli::category("Npm-related commands")]
pub struct Logout {
    /// Remove credentials for the registry configured for this scope
    #[cli::option("-s,--scope")]
    scope: Option<String>,

    /// Remove credentials for the publish registry
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
            let Some(ref config_path) = project.config.user_config_path else {
                return Err(Error::AuthenticationError("Failed to get user config path".to_string()));
            };

            let config_content = config_path
                .fs_read_text()?;

            let updated_content = if let Some(scope) = &self.scope {
                let scope = scope.strip_prefix('@').unwrap_or(scope);

                let updated = DataDocument::update_document_field(
                    &config_content,
                    zpm_parsers::Path::from_segments(vec![
                        "npmScopes".to_string(),
                        scope.to_string(),
                        "npmAuthToken".to_string(),
                    ]),
                    zpm_parsers::Value::Undefined,
                )?;

                let updated = DataDocument::update_document_field(
                    &updated,
                    zpm_parsers::Path::from_segments(vec![
                        "npmScopes".to_string(),
                        scope.to_string(),
                        "npmAuthIdent".to_string(),
                    ]),
                    zpm_parsers::Value::Undefined,
                )?;

                crate::report::if_active_async(|report| {
                    report.info(format!("Successfully logged out from scope {}", DataType::Scope.colorize(scope)));
                }).await;

                updated
            } else {
                let registry
                    = get_registry(&project.config, None, self.publish)?
                        .to_string();

                let updated = DataDocument::update_document_field(
                    &config_content,
                    zpm_parsers::Path::from_segments(vec![
                        "npmRegistries".to_string(),
                        registry.to_string(),
                        "npmAuthToken".to_string(),
                    ]),
                    zpm_parsers::Value::Undefined,
                )?;

                let updated = DataDocument::update_document_field(
                    &updated,
                    zpm_parsers::Path::from_segments(vec![
                        "npmRegistries".to_string(),
                        registry.to_string(),
                        "npmAuthIdent".to_string(),
                    ]),
                    zpm_parsers::Value::Undefined,
                )?;

                crate::report::if_active_async(|report| {
                    report.info(format!("Successfully logged out from {}", DataType::Url.colorize(&registry)));
                }).await;

                updated
            };

            config_path
                .fs_write_text(&updated_content)?;

            Ok(())
        }).await
    }
}
