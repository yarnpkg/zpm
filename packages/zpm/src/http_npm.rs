use std::collections::HashSet;
use std::sync::{LazyLock, Mutex};

use regex::{Captures, Regex};
use reqwest::Response;
use serde::Deserialize;
use tokio::time::{sleep, Duration};
use zpm_config::Configuration;
use zpm_parsers::JsonDocument;
use zpm_primitives::Ident;
use zpm_utils::DataType;

use crate::{
    error::Error,
    http::{HttpClient, HttpRequest},
    report::{current_report, PromptType},
};

static WARNED_REGISTRIES: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

pub struct NpmHttpParams<'a> {
    pub http_client: &'a HttpClient,
    pub registry: &'a str,
    pub path: &'a str,
    pub authorization: Option<&'a str>,
}

pub enum AuthorizationMode {
    RespectConfiguration,
    AlwaysAuthenticate,
    NeverAuthenticate,
    BestEffort,
}

macro_rules! scope_registry_setting {
    ($config:expr, $registry:expr, $ident:expr, $field:ident) => {
        (|| {
            if let Some(ident) = &$ident {
                if let Some(scope) = ident.scope() {
                    let scope_settings
                        = $config.settings.npm_scopes.get(scope);

                    if let Some(scope_settings) = scope_settings {
                        if let Some(value) = scope_settings.$field.value.as_ref() {
                            return Some(value);
                        }
                    }
                }
            }

            if let Some(registry_settings) = $config.settings.npm_registries.get($registry) {
                if let Some(value) = registry_settings.$field.value.as_ref() {
                    return Some(value);
                }
            }

            None
        })()
    }
}

fn get_registry_raw<'a>(config: &'a Configuration, scope: Option<&str>, publish: bool) -> Result<&'a str, Error> {
    if let Some(scope) = scope {
        let scope_settings
            = config.settings.npm_scopes.get(scope);

        if let Some(scope_settings) = scope_settings {
            if publish {
                let npm_publish_registry
                    = scope_settings.npm_publish_registry.value.as_ref().map(|s| s.as_str());

                if let Some(registry) = npm_publish_registry {
                    return Ok(registry);
                }
            }

            let npm_registry_server
                = scope_settings.npm_registry_server.value.as_ref().map(|s| s.as_str());

            if let Some(registry) = npm_registry_server {
                return Ok(registry);
            }
        }
    }

    if publish {
        let publish_registry
            = config.settings.npm_publish_registry.value.as_ref().map(|s| s.as_str());

        if let Some(registry) = publish_registry {
            return Ok(registry);
        }
    }

    let registry_server
        = config.settings.npm_registry_server.value.as_str();

    Ok(registry_server)
}

pub fn get_registry<'a>(config: &'a Configuration, scope: Option<&str>, publish: bool) -> Result<&'a str, Error> {
    let registry
        = get_registry_raw(config, scope, publish)?;

    Ok(registry.strip_suffix('/').unwrap_or(registry))
}


pub fn should_authenticate(config: &Configuration, registry: &str, ident: Option<&Ident>, auth_mode: AuthorizationMode) -> bool {
    match auth_mode {
        AuthorizationMode::RespectConfiguration => {
            *scope_registry_setting!(config, registry, ident, npm_always_auth)
                .unwrap_or(&config.settings.npm_always_auth.value)
        },

        AuthorizationMode::AlwaysAuthenticate | AuthorizationMode::BestEffort => {
            true
        },

        AuthorizationMode::NeverAuthenticate => {
            false
        },
    }
}

pub fn get_authorization(config: &Configuration, registry: &str, ident: Option<&Ident>, auth_mode: AuthorizationMode) -> Option<String> {
    let should_authenticate
        = should_authenticate(config, registry, ident, auth_mode);

    if !should_authenticate {
        return None;
    }

    let auth_token
        = scope_registry_setting!(config, registry, ident, npm_auth_token)
            .or_else(|| config.settings.npm_auth_token.value.as_ref());

    if let Some(auth_token) = auth_token {
        return Some(format!("Bearer {}", auth_token));
    }

    let auth_ident
        = scope_registry_setting!(config, registry, ident, npm_auth_ident)
            .or_else(|| config.settings.npm_auth_ident.value.as_ref());

    if let Some(auth_ident) = auth_ident {
        if auth_ident.contains(':') {
            return Some(format!("Basic {}", base64::Engine::encode(&base64::engine::general_purpose::STANDARD, auth_ident.as_bytes())));
        } else {
            return Some(format!("Basic {}", auth_ident));
        }
    }

    None
}

pub async fn get(params: &NpmHttpParams<'_>) -> Result<Response, Error> {
    let url
        = format!("{}{}", params.registry, params.path);
    let registry_base
        = params.registry.to_string();

    let fetch_future = async {
        let request
            = params.http_client.get(&url)?
                .header("authorization", params.authorization);
        Ok::<_, Error>(request.send().await?)
    };

    let warning_future = async {
        sleep(Duration::from_secs(15)).await;

        // Check if we should warn about this registry
        let should_warn
            = {
                let mut warned = WARNED_REGISTRIES.lock().unwrap();
                if !warned.contains(&registry_base) {
                    warned.insert(registry_base.clone());
                    true
                } else {
                    false
                }
            }; // Lock is dropped here

        if should_warn {
            current_report().await.as_mut().map(|report| {
                report.warn(format!("Requests to {} are taking suspiciously long...", registry_base));
            });
        }
    };

    let response
        = tokio::select! {
            result = fetch_future => result?,
            _ = warning_future => {
                // Warning was shown, now wait for the request to complete
                let request
                    = params.http_client.get(&url)?
                        .header("authorization", params.authorization);
                request.send().await?
            }
        };

    Ok(response)
}

pub async fn put(params: &NpmHttpParams<'_>, body: String) -> Result<Response, Error> {
    let url
        = format!("{}{}", params.registry, params.path);

    let mut request
        = params.http_client.put(url)?
            .enable_status_check(false)
            .header("content-type", Some("application/json"))
            .header("authorization", params.authorization)
            .body(body);

    let mut response
        = request
            .try_clone()
            .expect("Failed to clone request")
            .send()
            .await?;

    if is_otp_error(&response) {
        render_otp_notice(&response).await;

        let otp
            = ask_for_otp().await?;

        request = inject_otp_headers(request, otp);
        response = request.send().await?;
    }

    handle_invalid_authentication_error(params, &response).await?;

    Ok(response.error_for_status()?)
}

fn inject_otp_headers(request: HttpRequest<'_>, otp: String) -> HttpRequest<'_> {
    request.header("npm-otp", Some(otp))
}

async fn handle_invalid_authentication_error(params: &NpmHttpParams<'_>, response: &Response) -> Result<(), Error> {
    if is_otp_error(response) {
        return Err(Error::AuthenticationError(
            "Invalid OTP token".to_string()
        ));
    }

    if response.status().as_u16() == 401 {
        let whoami = match params.authorization {
            Some(authorization) => {
                whoami(params, authorization).await.unwrap_or_else(|_| "an unknown user".to_string())
            },

            None => {
                "an anonymous user".to_string()
            },
        };

        if whoami.contains("npm_default") {
            return Err(Error::AuthenticationError(
                format!("Invalid authentication (as {})", whoami)
            ));
        }
    }

    Ok(())
}

fn is_otp_error(response: &Response) -> bool {
    let www_authenticate
        = response.headers()
            .get("www-authenticate");

    if let Some(www_authenticate) = www_authenticate {
        if let Ok(www_authenticate_value) = www_authenticate.to_str() {
            return www_authenticate_value.split(",").any(|s| s.trim() == "OTP");
        }
    }

    false
}

async fn whoami(params: &NpmHttpParams<'_>, authorization: &str) -> Result<String, Error> {
    let http_client
        = params.http_client;

    let response = http_client
        .get(format!("{}/-/whoami", params.registry))?
        .header("authorization", Some(authorization))
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(Error::AuthenticationError(format!(
            "Failed to get user info: {}",
            response.status()
        )));
    }

    #[derive(Deserialize)]
    struct WhoamiResponse {
        username: String,
    }

    let body
        = response.text().await?;
    let data: WhoamiResponse
        = JsonDocument::hydrate_from_str(&body)?;

    Ok(data.username)
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

async fn ask_for_otp() -> Result<String, Error> {
    if std::env::var("YARN_IS_TEST_ENV").is_ok() {
        return Ok(std::env::var("YARN_INJECT_NPM_2FA_TOKEN").unwrap_or_default());
    }

    let otp = current_report().await.as_mut()
        .map(|report| report.prompt(PromptType::Input("One-time password".to_string())))
        .unwrap()
        .await;

    Ok(otp)
}
