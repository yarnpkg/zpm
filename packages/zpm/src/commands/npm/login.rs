use clipanion::cli;
use serde::{Deserialize, Serialize};
use zpm_utils::{DataType, QueryString};

use crate::{
    error::Error,
    http::HttpClient,
    http_npm::{self, get_registry, NpmHttpParams},
    project::Project,
    report::{current_report, with_report_result, PromptType, StreamReport, StreamReportConfig},
};

#[cli::command]
#[cli::path("npm", "login")]
#[cli::category("Npm-related commands")]
#[cli::description("Store new login info to access the npm registry")]
pub struct Login {
    #[cli::option("-s,--scope")]
    #[cli::description("Login to the registry configured for a given scope")]
    scope: Option<String>,

    #[cli::option("--publish", default = false)]
    #[cli::description("Login to the publish registry")]
    publish: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct NpmLoginPayload {
    #[serde(rename = "_id")]
    id: String,
    #[serde(skip_serializing_if = "Option::is_none", rename = "_rev")]
    rev: Option<String>,
    name: String,
    password: String,
    user_type: String,
    roles: Vec<String>,
    date: String,
}

#[derive(Deserialize)]
struct NpmLoginResponse {
    token: String,
    #[allow(dead_code)]
    ok: Option<bool>,
}

struct Credentials {
    username: String,
    password: String,
}

impl Login {
    #[tokio::main]
    pub async fn execute(&self) -> Result<(), Error> {
        let project
            = Project::new(None).await?;

        let registry
            = get_registry(&project.config, self.scope.as_deref(), self.publish)?
                .to_string();

        let report = StreamReport::new(StreamReportConfig {
            ..StreamReportConfig::from_config(&project.config)
        });

        with_report_result(report, async {
            current_report().await.as_mut().map(|report| {
                report.info(format!("Logging in to {}", DataType::Url.colorize(&registry)));
            });

            let is_github = registry.contains("npm.pkg.github.com");
            if is_github {
                println!("Note: You seem to be using the GitHub Package Registry.");
                println!("Tokens must be generated with the 'repo', 'write:packages', and 'read:packages' permissions.");
            }

            let credentials
                = get_credentials(is_github).await?;

            // Authenticate with registry
            let token = self.register_or_login(
                &project.http_client,
                &registry,
                &credentials.username,
                &credentials.password,
            ).await?;

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
                zpm_parsers::Value::String(token),
            )?;

            config_path
                .fs_write_text(&updated_content)?;

            current_report().await.as_mut().map(|report| {
                report.info("Successfully logged in".to_string());
            });

            Ok(())
        }).await
    }

    async fn register_or_login(&self, http_client: &HttpClient, registry: &str, username: &str, password: &str) -> Result<String, Error> {
        let user_id
            = format!("org.couchdb.user:{}", QueryString::encode(username));
        let user_url
            = format!("/-/user/{}", user_id);

        let payload = sonic_rs::to_string(&NpmLoginPayload {
            id: format!("org.couchdb.user:{}", username),
            name: username.to_string(),
            password: password.to_string(),
            user_type: "user".to_string(),
            roles: vec![],
            date: chrono::Utc::now().to_rfc3339(),
            rev: None,
        })?;

        let response = http_npm::put(&NpmHttpParams {
            http_client,
            registry,
            path: user_url.as_str(),
            authorization: None,
        }, payload).await?;

        let body = response.text().await
            .map_err(|e| Error::AuthenticationError(format!("Failed to read response: {}", e)))?;

        let login_response: NpmLoginResponse = sonic_rs::from_str(&body)
            .map_err(|e| Error::AuthenticationError(format!("Failed to parse response: {}", e)))?;

        return Ok(login_response.token);
    }
}

async fn get_credentials(is_token: bool) -> Result<Credentials, Error> {
    if std::env::var("YARN_IS_TEST_ENV").is_ok() {
        return Ok(Credentials {
            username: std::env::var("YARN_INJECT_NPM_USER").unwrap_or_default(),
            password: std::env::var("YARN_INJECT_NPM_PASSWORD").unwrap_or_default(),
        });
    }

    let mut report_guard
        = current_report().await;

    let report
        = report_guard.as_mut()
            .expect("No report set");

    let username
        = report.prompt(PromptType::Input("Username".to_string())).await;

    let password
        = report.prompt(PromptType::Password(if is_token {"Token"} else {"Password"}.to_string())).await;

    Ok(Credentials {
        username,
        password,
    })
}
