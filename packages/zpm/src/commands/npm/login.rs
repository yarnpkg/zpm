use std::sync::LazyLock;

use base64::Engine;
use clipanion::cli;
use http::Method;
use regex::{Captures, Regex};
use reqwest::Response;
use serde::{Deserialize, Serialize};
use sonic_rs::{JsonContainerTrait, JsonValueMutTrait, JsonValueTrait};
use zpm_utils::DataType;

use crate::{
    error::Error,
    http::{HttpClient, HttpRequest, HttpRequestParams},
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
            = self.get_registry(&project)?
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
                = self.get_credentials(is_github).await?;

            // Authenticate with registry
            let token = self.register_or_login(
                &registry,
                &credentials.username,
                &credentials.password,
                &project.http_client,
            ).await?;

            let Some(config_path) = project.config.user.path else {
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

    fn get_registry<'a>(&self, project: &'a Project) -> Result<&'a str, Error> {
        if let Some(scope) = &self.scope {
            let scope_settings
                = project.config.project.npm_scopes.value.get(scope);

            if let Some(scope_settings) = scope_settings {
                if self.publish {
                    let npm_publish_registry
                        = scope_settings.npm_publish_registry.as_ref().map(|s| s.as_str());

                    if let Some(registry) = npm_publish_registry {
                        return Ok(registry);
                    }
                }

                let npm_registry_server
                    = scope_settings.npm_registry_server.as_ref().map(|s| s.as_str());

                if let Some(registry) = npm_registry_server {
                    return Ok(registry);
                }
            }
        }

        if self.publish {
            let publish_registry
                = project.config.project.npm_publish_registry.value.as_ref().map(|s| s.as_str());

            if let Some(registry) = publish_registry {
                return Ok(registry);
            }
        }

        let registry_server
            = project.config.project.npm_registry_server.value.as_str();

        Ok(registry_server)
    }

    async fn get_credentials(&self, is_token: bool) -> Result<Credentials, Error> {
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

    async fn register_or_login(&self, registry: &str, username: &str, password: &str, http_client: &HttpClient) -> Result<String, Error> {
        // Registration and login are both handled as a `put` by npm. Npm uses a lax
        // endpoint as of 2023-11 where there are no conflicts if the user already
        // exists, but some registries such as Verdaccio are stricter and return a
        // `409 Conflict` status code for existing users. In this case, the client
        // should put a user revision for this specific session (with basic HTTP
        // auth).

        let user_url = format!(
            "{}/-/user/org.couchdb.user:{}",
            registry.trim_end_matches('/'),
            urlencoding::encode(username)
        );

        let payload = NpmLoginPayload {
            id: format!("org.couchdb.user:{}", username),
            name: username.to_string(),
            password: password.to_string(),
            user_type: "user".to_string(),
            roles: vec![],
            date: chrono::Utc::now().to_rfc3339(),
            rev: None,
        };

        // The request shouldn't always fail; we want to handle 409 ourselves.
        let lax_params = HttpRequestParams {
            strict: false,
            ..Default::default()
        };

        let request = http_client
            .request(&user_url, Method::PUT, lax_params)?
            .header("content-type", "application/json")
            .body(sonic_rs::to_string(&payload).unwrap());

        let response
            = send_with_otp(request)
                .await?;

        if response.status().is_success() {
            let body = response.text().await
                .map_err(|e| Error::AuthenticationError(format!("Failed to read response: {}", e)))?;

            let login_response: NpmLoginResponse = sonic_rs::from_str(&body)
                .map_err(|e| Error::AuthenticationError(format!("Failed to parse response: {}", e)))?;

            return Ok(login_response.token);
        }

        if response.status().as_u16() == 409 {
            return self.authenticate_with_basic_auth(
                &user_url,
                username,
                password,
                http_client,
            ).await;
        }

        response.error_for_status()?;
        unreachable!()
    }

    async fn authenticate_with_basic_auth(&self, user_url: &str, username: &str, password: &str, http_client: &HttpClient) -> Result<String, Error> {
        let mut new_payload = sonic_rs::to_value(&NpmLoginPayload {
            id: format!("org.couchdb.user:{}", username),
            name: username.to_string(),
            password: password.to_string(),
            user_type: "user".to_string(),
            roles: vec![],
            date: chrono::Utc::now().to_rfc3339(),
            rev: None,
        })?;

        let new_payload_map = new_payload
            .as_object_mut()
            .unwrap();

        let auth
            = format!("{}:{}", username, password);
        let auth_header
            = format!("Basic {}", base64::engine::general_purpose::STANDARD.encode(auth.as_bytes()));

        let user_info_response = http_client
            .get(user_url)?
            .header("authorization", &auth_header)
            .send()
            .await
            .map_err(|e| Error::AuthenticationError(format!("Failed to get user info: {}", e)))?;

        if !user_info_response.status().is_success() {
            return Err(Error::AuthenticationError(format!(
                "Failed to get user info: {}",
                user_info_response.status()
            )));
        }

        let body = user_info_response.text().await
            .map_err(|e| Error::AuthenticationError(format!("Failed to read user info: {}", e)))?;

        let user_info: sonic_rs::Value = sonic_rs::from_str(&body)
            .map_err(|e| Error::AuthenticationError(format!("Failed to parse user info: {}", e)))?;

        if let Some(user_info_map) = user_info.as_object() {
            for (key, value) in user_info_map {
                if !new_payload_map.contains_key(&key) || key == "roles" {
                    new_payload_map.insert(key, value.clone());
                }
            }
        }

        let rev = new_payload_map["_rev"]
            .as_str()
            .ok_or_else(|| Error::AuthenticationError("No revision found in user info".to_string()))?
            .to_string();

        let revision_url
            = format!("{}/{}", user_url, rev);

        let response = http_client
            .put(&revision_url)?
            .header("content-type", "application/json")
            .header("authorization", &auth_header)
            .body(sonic_rs::to_string(&new_payload).unwrap())
            .send()
            .await
            .map_err(|e| Error::AuthenticationError(format!("Failed to update user: {}", e)))?;

        if !response.status().is_success() {
            return Err(Error::AuthenticationError(format!(
                "Failed to update user: {}",
                response.status()
            )));
        }

        let body = response.text().await
            .map_err(|e| Error::AuthenticationError(format!("Failed to read response: {}", e)))?;

        let login_response: NpmLoginResponse = sonic_rs::from_str(&body)
            .map_err(|e| Error::AuthenticationError(format!("Failed to parse response: {}", e)))?;

        Ok(login_response.token)
    }
}

fn is_otp_error(response: &Response) -> bool {
    let www_authenticate
        = response.headers()
            .get("www-authenticate");

    if let Some(www_authenticate) = www_authenticate {
        if let Ok(www_authenticate_value) = www_authenticate.to_str() {
            return www_authenticate_value.to_ascii_lowercase().split(",").any(|s| s.trim() == "otp");
        }
    }

    false
}

static URL_REGEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"https?://[^ ]+").unwrap());

async fn render_otp_notice(response: &Response) {
    let notice
        = response.headers()
            .get("npm-notice")
            .map(|notice| notice.to_str())
            .transpose()
            .unwrap_or_default();

    if let Some(notice) = notice {
        let formatted_notice = URL_REGEX.replace(notice, |caps: &Captures| {
            DataType::Url.colorize(caps.get(0).unwrap().as_str()).to_string()
        });

        current_report().await.as_mut()
            .map(|report| report.info(formatted_notice.to_string()));
    }
}

async fn query_otp() -> Result<String, Error> {
    if std::env::var("YARN_IS_TEST_ENV").is_ok() {
        return Ok(std::env::var("YARN_INJECT_NPM_2FA_TOKEN").unwrap_or_default());
    }

    let otp = current_report().await.as_mut()
        .map(|report| report.prompt(PromptType::Input("One-time password".to_string())))
        .unwrap()
        .await;

    Ok(otp)
}

async fn send_with_otp(request: HttpRequest<'_>) -> Result<Response, Error> {
    let mut response
        = request.try_clone()
            .expect("Failed to clone request")
            .send()
            .await?;

    if is_otp_error(&response) {
        render_otp_notice(&response).await;

        response = request
            .try_clone()
            .expect("Failed to clone request")
            .header("npm-otp", query_otp().await?)
            .send()
            .await?;
    }

    Ok(response)
}
