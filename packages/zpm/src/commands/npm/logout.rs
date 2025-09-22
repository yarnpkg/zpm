use clipanion::cli;
use serde::Deserialize;
use zpm_primitives::Ident;
use zpm_utils::FromFileString;

use crate::{
    error::Error,
    http_npm::{self, get_authorization, get_registry, AuthorizationMode, NpmHttpParams},
    project::Project, report::current_report,
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
            report.info("Successfully logged in".to_string());
        });

        Ok(())
    }
}
