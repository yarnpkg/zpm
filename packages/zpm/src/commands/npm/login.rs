use std::time::Duration;

use clipanion::cli;
use http::StatusCode;
use serde::{Deserialize, Serialize};
use tokio::time::sleep;
use zpm_parsers::JsonDocument;
use zpm_utils::{DataType, IoResultExt, QueryString};

use crate::{
    error::Error,
    http::HttpClient,
    http_npm::{self, get_registry, NpmHttpParams},
    project::Project,
    report::{current_report, with_report_result, PromptType, StreamReport, StreamReportConfig},
};

const WEB_LOGIN_REGISTRIES: &[&str] = &[
    "https://registry.npmjs.org",
    "https://registry.yarnpkg.com",
];

/// Login to the npm registry
///
/// This command will ask you for your username, password, and 2FA One-Time-Password (when it applies). It will then modify your local configuration (in your home folder, never in the project itself) to reference the new tokens thus generated.
///
/// Adding the `-s,--scope` flag will cause the authentication to be done against whatever registry is configured for the associated scope (see also `npmScopes`).
///
/// Adding the `--publish` flag will cause the authentication to be done against the registry used when publishing the package (see also `publishConfig.registry` and `npmPublishRegistry`).
///
#[cli::command]
#[cli::path("npm", "login")]
#[cli::category("Npm-related commands")]
pub struct Login {
    /// Login to the registry configured for a given scope
    #[cli::option("-s,--scope")]
    scope: Option<String>,

    /// Login to the publish registry
    #[cli::option("--publish", default = false)]
    publish: bool,

    /// Enable web-based login
    #[cli::option("--web-login")]
    web_login: Option<bool>,
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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NpmWebLoginInitResponse {
    login_url: String,
    done_url: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NpmWebLoginCheckResponse {
    token: String,
}

enum NpmWebLoginState {
    Waiting(Duration),
    Done(String),
}

struct Credentials {
    username: String,
    password: String,
}

impl Login {
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
            current_report().await.as_ref().map(|report| {
                report.info(format!("Logging in to {}", DataType::Url.colorize(&registry)));
            });

            let token
                = self.authenticate(&project.http_client, &registry).await?;

            let Some(config_path) = project.config.user_config_path else {
                return Err(Error::AuthenticationError("Failed to get user config path".to_string()));
            };

            let config_content = config_path
                .fs_read_text()
                .ok_missing()?
                .unwrap_or_default();

            let auth_token_path = if let Some(scope) = self.scope.as_ref() {
                zpm_parsers::Path::from_segments(vec![
                    "npmScopes".to_string(),
                    scope.to_string(),
                    "npmAuthToken".to_string(),
                ])
            } else {
                zpm_parsers::Path::from_segments(vec![
                    "npmRegistries".to_string(),
                    registry.to_string(),
                    "npmAuthToken".to_string(),
                ])
            };

            let updated_content = zpm_parsers::yaml::Yaml::update_document_field(
                &config_content,
                auth_token_path,
                zpm_parsers::Value::String(token),
            )?;

            config_path
                .fs_write_text(&updated_content)?;

            current_report().await.as_ref().map(|report| {
                report.info("Successfully logged in".to_string());
            });

            Ok(())
        }).await
    }

    async fn authenticate(&self, http_client: &HttpClient, registry: &str) -> Result<String, Error> {
        let enable_web_login
            = self.web_login
                .unwrap_or_else(|| WEB_LOGIN_REGISTRIES.contains(&registry));

        if enable_web_login {
            let token = self.login_via_web(
                &http_client,
                &registry,
            ).await?;

            if let Some(token) = token {
                return Ok(token);
            }
        }

        let is_github
            = registry.contains("npm.pkg.github.com");

        if is_github {
            println!("Note: You seem to be using the GitHub Package Registry.");
            println!("Tokens must be generated with the 'repo', 'write:packages', and 'read:packages' permissions.");
        }

        let credentials
            = get_credentials(is_github).await?;

        // Authenticate with registry
        let token = self.register_or_login(
            &http_client,
            &registry,
            &credentials.username,
            &credentials.password,
        ).await?;

        Ok(token)
    }

    async fn web_login_init(&self, http_client: &HttpClient, registry: &str) -> Result<Option<NpmWebLoginInitResponse>, Error> {
        let response = http_client
            .post(format!("{}/-/v1/login", registry))?
            .header("npm-auth-type", Some("web"))
            .enable_status_check(false)
            .send()
            .await?;

        if !response.status().is_success() {
            return Ok(None);
        }

        let login_body
            = response.text().await
                .map_err(|e| Error::AuthenticationError(format!("Failed to read response: {}", e)))?;

        let login_response
            = JsonDocument::hydrate_from_str(&login_body)
                .map_err(|e| Error::AuthenticationError(format!("Failed to parse response: {}", e)))?;

        Ok(Some(login_response))
    }

    async fn web_login_check(&self, http_client: &HttpClient, done_url: &str) -> Result<NpmWebLoginState, Error> {
        let response = http_client
            .get(done_url)?
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(Error::AuthenticationError(format!(
                "Failed to retrieve the login token: {}",
                response.status()
            )));
        }

        if response.status() == StatusCode::ACCEPTED {
            let retry_duration
                = response.headers().get("retry-after")
                    .and_then(|value| value.to_str().ok())
                    .and_then(|value| value.parse::<u64>().ok())
                    .unwrap_or(1);

            return Ok(NpmWebLoginState::Waiting(Duration::from_secs(retry_duration)));
        }

        let done_body
            = response.text().await
                .map_err(|e| Error::AuthenticationError(format!("Failed to read response: {}", e)))?;

        let done_response: NpmWebLoginCheckResponse
            = JsonDocument::hydrate_from_str(&done_body)
                .map_err(|e| Error::AuthenticationError(format!("Failed to parse response: {}", e)))?;

        Ok(NpmWebLoginState::Done(done_response.token))
    }

    async fn login_via_web(&self, http_client: &HttpClient, registry: &str) -> Result<Option<String>, Error> {
        let Some(login_response) = self.web_login_init(http_client, registry).await? else {
            return Ok(None);
        };

        open::that(login_response.login_url)?;

        loop {
            let check
                = self.web_login_check(http_client, login_response.done_url.as_str()).await?;

            match check {
                NpmWebLoginState::Waiting(duration) => {
                    sleep(duration).await;
                },

                NpmWebLoginState::Done(token) => {
                    return Ok(Some(token));
                },
            }
        }
    }

    async fn register_or_login(&self, http_client: &HttpClient, registry: &str, username: &str, password: &str) -> Result<String, Error> {
        let user_id
            = format!("org.couchdb.user:{}", QueryString::encode(username));
        let user_url
            = format!("/-/user/{}", user_id);

        let payload = JsonDocument::to_string(&NpmLoginPayload {
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
            otp: None,
        }, payload).await?;

        let body
            = response.text().await
                .map_err(|e| Error::AuthenticationError(format!("Failed to read response: {}", e)))?;

        let login_response: NpmLoginResponse
            = JsonDocument::hydrate_from_str(&body)
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

    let report_guard
        = current_report().await;

    let report
        = report_guard.as_ref()
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
