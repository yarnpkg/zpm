use std::collections::HashSet;
use std::sync::{Arc, LazyLock};

use bytes::Bytes;
use dashmap::DashMap;
use regex::{Captures, Regex};
use reqwest::Response;
use serde::Deserialize;
use sha2::{Sha256, Digest};
use tokio::sync::OnceCell;
use zpm_config::Configuration;
use zpm_parsers::JsonDocument;
use zpm_primitives::Ident;
use zpm_utils::{DataType, Path};

use crate::{
    error::Error,
    http::{HttpClient, HttpRequest},
    npm,
    report::{current_report, PromptType},
};

pub struct NpmHttpParams<'a> {
    pub http_client: &'a HttpClient,
    pub registry: &'a str,
    pub path: &'a str,
    pub authorization: Option<&'a str>,
    pub otp: Option<&'a str>,
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
                    let scope_key = scope.strip_prefix('@').unwrap_or(scope);
                    let scope_settings
                        = $config.settings.npm_scopes.get(scope_key);

                    if let Some(scope_settings) = scope_settings {
                        if let Some(value) = scope_settings.$field.value.as_ref() {
                            return Some(value);
                        }
                    }
                }
            }

            // Try exact match first
            if let Some(registry_settings) = $config.settings.npm_registries.get($registry) {
                if let Some(value) = registry_settings.$field.value.as_ref() {
                    return Some(value);
                }
            }

            // Also try with trailing slash (for normalization)
            let registry_with_slash = format!("{}/", $registry);
            if let Some(registry_settings) = $config.settings.npm_registries.get(&registry_with_slash) {
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
            = config.settings.npm_scopes.get(scope.strip_prefix('@').unwrap_or(scope));

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

pub struct GetAuthorizationOptions<'a> {
    pub configuration: &'a Configuration,
    pub http_client: &'a HttpClient,
    pub registry: &'a str,
    pub ident: Option<&'a Ident>,
    pub auth_mode: AuthorizationMode,
    pub allow_oidc: bool,
}

pub fn should_authenticate(options: &GetAuthorizationOptions<'_>) -> bool {
    match options.auth_mode {
        AuthorizationMode::RespectConfiguration => {
            // For scoped packages, always authenticate if credentials are available
            let is_scoped = options.ident
                .map(|ident| ident.scope().is_some())
                .unwrap_or(false);

            if is_scoped {
                return true;
            }

            // For unscoped packages, only authenticate if npmAlwaysAuth is true
            *scope_registry_setting!(options.configuration, options.registry, options.ident, npm_always_auth)
                .unwrap_or(&options.configuration.settings.npm_always_auth.value)
        },

        AuthorizationMode::AlwaysAuthenticate | AuthorizationMode::BestEffort => {
            true
        },

        AuthorizationMode::NeverAuthenticate => {
            false
        },
    }
}

pub struct GetIdTokenOptions<'a> {
    pub http_client: &'a HttpClient,
    pub audience: &'a str,
}

fn get_npm_audience(registry: &str) -> Result<String, Error> {
    let registry_url
        = url::Url::parse(registry)?;

    let registry_host
        = registry_url.host_str()
            .expect("\"http:\" URL should have a host");

    Ok(format!("npm:{}", registry_host))
}

pub async fn get_id_token(options: &GetIdTokenOptions<'_>) -> Result<Option<String>, Error> {
    if let Ok(oidc_token) = std::env::var("NPM_ID_TOKEN") {
        return Ok(Some(oidc_token));
    }

    let Ok(actions_id_token_request_url) = std::env::var("ACTIONS_ID_TOKEN_REQUEST_URL") else {
        return Ok(None);
    };

    let Ok(actions_id_token_request_token) = std::env::var("ACTIONS_ID_TOKEN_REQUEST_TOKEN") else {
        return Ok(None);
    };

    let mut actions_id_token_request_url
        = url::Url::parse(&actions_id_token_request_url)?;

    actions_id_token_request_url.query_pairs_mut()
        .append_pair("audience", options.audience);

    let response
        = options.http_client.get(actions_id_token_request_url)?
            .header("authorization", Some(format!("Bearer {}", actions_id_token_request_token)))
            .send()
            .await?;

    let body
        = response.text().await?;

    #[derive(Deserialize)]
    struct ActionsIdTokenResponse {
        value: String,
    }

    let data: ActionsIdTokenResponse
        = JsonDocument::hydrate_from_str(&body)?;

    Ok(Some(data.value))
}

fn get_ident_url(ident: &Ident) -> String {
    let (scope, name)
        = ident.split();

    if let Some(scope) = scope {
        format!("{}%2f{}", scope, name)
    } else {
        name.to_string()
    }
}

async fn get_oidc_token(options: &GetAuthorizationOptions<'_>) -> Result<Option<String>, Error> {
    let Some(ident) = options.ident else {
        return Ok(None);
    };

    let id_token
        = get_id_token(&GetIdTokenOptions {
            http_client: options.http_client,
            audience: &get_npm_audience(&options.registry)?,
        }).await?;

    let Some(id_token) = id_token else {
        return Ok(None);
    };

    let response
        = options.http_client.post(format!("{}/-/npm/v1/oidc/token/exchange/package/{}", options.registry, get_ident_url(ident)))?
            .header("authorization", Some(format!("Bearer {}", id_token)))
            .send()
            .await?;

    #[derive(Deserialize)]
    struct OidcTokenResponse {
        token: String,
    }

    let body
        = response.text().await?;

    let data: OidcTokenResponse
        = JsonDocument::hydrate_from_str(&body)?;

    Ok(Some(data.token))
}

/// Returns whether authentication is strictly required (should error if not configured).
/// This is different from should_authenticate, which returns whether auth should be sent if available.
fn is_auth_strictly_required(options: &GetAuthorizationOptions<'_>) -> bool {
    match options.auth_mode {
        AuthorizationMode::AlwaysAuthenticate => true,

        AuthorizationMode::RespectConfiguration => {
            // For scoped packages, auth is preferred but not strictly required
            // (the server may accept anonymous requests for public packages)
            let is_scoped = options.ident
                .map(|ident| ident.scope().is_some())
                .unwrap_or(false);

            if is_scoped {
                return false;
            }

            // For unscoped packages, npmAlwaysAuth=true means auth is strictly required
            *scope_registry_setting!(options.configuration, options.registry, options.ident, npm_always_auth)
                .unwrap_or(&options.configuration.settings.npm_always_auth.value)
        },

        AuthorizationMode::BestEffort | AuthorizationMode::NeverAuthenticate => false,
    }
}

pub async fn get_authorization(options: &GetAuthorizationOptions<'_>) -> Result<Option<String>, Error> {
    let should_authenticate
        = should_authenticate(options);

    if !should_authenticate {
        return Ok(None);
    }

    let auth_token
        = scope_registry_setting!(options.configuration, options.registry, options.ident, npm_auth_token)
            .or_else(|| options.configuration.settings.npm_auth_token.value.as_ref());

    if let Some(auth_token) = auth_token {
        return Ok(Some(format!("Bearer {}", auth_token.value)));
    }

    let auth_ident
        = scope_registry_setting!(options.configuration, options.registry, options.ident, npm_auth_ident)
            .or_else(|| options.configuration.settings.npm_auth_ident.value.as_ref());

    if let Some(auth_ident) = auth_ident {
        if auth_ident.contains(':') {
            return Ok(Some(format!("Basic {}", base64::Engine::encode(&base64::engine::general_purpose::STANDARD, auth_ident.as_bytes()))));
        } else {
            return Ok(Some(format!("Basic {}", auth_ident)));
        }
    }

    if options.allow_oidc {
        let oidc_token
            = get_oidc_token(options).await?;

        if let Some(oidc_token) = oidc_token {
            return Ok(Some(format!("Bearer {}", oidc_token)));
        }
    }

    // If auth is strictly required but no credentials were found, error
    if is_auth_strictly_required(options) {
        return Err(Error::AuthenticationError(
            "No authentication configured for request".to_string()
        ));
    }

    Ok(None)
}

pub async fn get(params: &NpmHttpParams<'_>) -> Result<Bytes, Error> {
    let url
        = format!("{}{}", params.registry, params.path);

    let bytes = match params.authorization {
        Some(authorization) => {
            let response = params.http_client.get(&url)?
                .header("authorization", Some(authorization))
                .enable_status_check(false)
                .send().await?;

            handle_invalid_authentication_error(params, &response).await?;

            response.error_for_status()?.bytes().await?
        },

        None => {
            params.http_client.cached_get(&url).await?
        },
    };

    Ok(bytes)
}

const CACHED_VERSION_FIELDS: &[&str] = &[
    "name", "version", "dist", "bin", "scripts",
    "os", "cpu", "libc",
    "dependencies", "dependenciesMeta", "optionalDependencies",
    "peerDependencies", "peerDependenciesMeta",
];

const CACHED_TOP_LEVEL_FIELDS: &[&str] = &["dist-tags", "time", "versions"];

static METADATA_CACHE_KEY: LazyLock<String> = LazyLock::new(|| {
    let mut hasher = Sha256::new();
    for field in CACHED_VERSION_FIELDS {
        hasher.update(field.as_bytes());
    }
    hex::encode(&hasher.finalize()[..3])
});

static METADATA_CACHE: LazyLock<DashMap<String, Arc<OnceCell<Result<Bytes, Error>>>>> = LazyLock::new(DashMap::new);

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
struct CachedMetadataDisk {
    metadata: Vec<u8>,
    etag: Option<String>,
    last_modified: Option<String>,
}

pub struct GetPackageMetadataParams<'a> {
    pub http_client: &'a HttpClient,
    pub registry: &'a str,
    pub ident: &'a Ident,
    pub authorization: Option<&'a str>,
    pub global_folder: &'a Path,
    pub refresh_lockfile: bool,
}

pub async fn get_package_metadata(params: &GetPackageMetadataParams<'_>) -> Result<Bytes, Error> {
    let cache_key
        = params.ident.to_string();

    let cell = METADATA_CACHE
        .entry(cache_key)
        .or_insert_with(|| Arc::new(OnceCell::new()))
        .clone();

    let result = cell.get_or_init(|| async {
        fetch_metadata_with_disk_cache(params).await
    }).await;

    result.clone()
}

fn get_cache_file_path(global_folder: &Path, registry: &str, ident: &Ident) -> Path {
    let registry_hostname
        = url::Url::parse(registry)
            .ok()
            .and_then(|u| u.host_str().map(|s| s.to_string()))
            .unwrap_or_else(|| "unknown".to_string());

    global_folder
        .with_join_str("metadata/npm")
        .with_join_str(&*METADATA_CACHE_KEY)
        .with_join_str(&registry_hostname)
        .with_join_str(&format!("{}.bin", ident.slug()))
}

fn trim_metadata(raw: &[u8]) -> Vec<u8> {
    let Ok(mut metadata) = serde_json::from_slice::<serde_json::Value>(raw) else {
        return raw.to_vec();
    };

    let top_level_fields: HashSet<&str>
        = CACHED_TOP_LEVEL_FIELDS.iter().copied().collect();
    let version_fields: HashSet<&str>
        = CACHED_VERSION_FIELDS.iter().copied().collect();

    if let Some(obj) = metadata.as_object_mut() {
        obj.retain(|k, _| top_level_fields.contains(k.as_str()));
    }

    if let Some(versions) = metadata.get_mut("versions").and_then(|v| v.as_object_mut()) {
        for version_data in versions.values_mut() {
            if let Some(obj) = version_data.as_object_mut() {
                obj.retain(|k, _| version_fields.contains(k.as_str()));
            }
        }
    }

    serde_json::to_vec(&metadata).unwrap_or_else(|_| raw.to_vec())
}

fn write_cache_to_disk(cache_file: &Path, metadata: &[u8], etag: Option<String>, last_modified: Option<String>) {
    let cache_dir
        = cache_file.dirname().unwrap();

    let _ = cache_dir.fs_create_dir_all();

    let entry = CachedMetadataDisk {
        metadata: trim_metadata(metadata),
        etag,
        last_modified,
    };

    let Ok(encoded) = rkyv::to_bytes::<rkyv::rancor::BoxedError>(&entry) else {
        return;
    };

    let _ = cache_file.fs_write_atomic(|tmp| {
        tmp.fs_write(&encoded)?;
        Ok::<_, zpm_utils::PathError>(())
    });
}

async fn fetch_metadata_with_disk_cache(params: &GetPackageMetadataParams<'_>) -> Result<Bytes, Error> {
    let registry_path
        = npm::registry_url_for_all_versions(params.ident);
    let url
        = format!("{}{}", params.registry, registry_path);

    let cache_file
        = get_cache_file_path(params.global_folder, params.registry, params.ident);

    let cached = if !params.refresh_lockfile {
        cache_file.fs_read_prealloc().ok()
            .and_then(|data| rkyv::from_bytes::<CachedMetadataDisk, rkyv::rancor::BoxedError>(&data).ok())
    } else {
        None
    };

    let mut request
        = params.http_client.get(&url)?
            .enable_status_check(false);

    if let Some(auth) = params.authorization {
        request = request.header("authorization", Some(auth));
    }

    if let Some(ref cached) = cached {
        if let Some(ref etag) = cached.etag {
            request = request.header("if-none-match", Some(etag.as_str()));
        }
        if let Some(ref last_modified) = cached.last_modified {
            request = request.header("if-modified-since", Some(last_modified.as_str()));
        }
    }

    let response
        = request.send().await?;

    if params.authorization.is_some() {
        let npm_params = NpmHttpParams {
            http_client: params.http_client,
            registry: params.registry,
            path: &registry_path,
            authorization: params.authorization,
            otp: None,
        };
        handle_invalid_authentication_error(&npm_params, &response).await?;
    }

    if response.status().as_u16() == 304 {
        if let Some(cached) = cached {
            return Ok(Bytes::from(cached.metadata));
        }
    }

    let etag
        = response.headers().get("etag")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
    let last_modified
        = response.headers().get("last-modified")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

    let body
        = response.error_for_status()?.bytes().await?;

    let body_for_disk = body.clone();
    let cache_file_for_disk = cache_file.clone();
    tokio::task::spawn_blocking(move || {
        write_cache_to_disk(&cache_file_for_disk, &body_for_disk, etag, last_modified);
    });

    Ok(body)
}

pub async fn post(params: &NpmHttpParams<'_>, body: String) -> Result<Response, Error> {
    let url
        = format!("{}{}", params.registry, params.path);

    let mut request
        = params.http_client.post(url)?
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
        let otp
            = ask_for_otp(params, &response).await?;

        request = inject_otp_headers(request, otp);
        response = request.send().await?;
    }

    handle_invalid_authentication_error(params, &response).await?;

    Ok(response.error_for_status()?)
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
        let otp
            = ask_for_otp(params, &response).await?;

        request = inject_otp_headers(request, otp);
        response = request.send().await?;
    }

    handle_invalid_authentication_error(params, &response).await?;

    if let Err(error) = response.error_for_status_ref() {
        let body
            = response.text().await?;

        #[derive(Deserialize)]
        struct ErrorResponse {
            error: String,
        }

        let message
            = JsonDocument::hydrate_from_str::<ErrorResponse>(&body)
                .ok()
                .map(|data| data.error);

        return Err(Error::HttpError { inner: Arc::new(error), extra: message });
    }

    Ok(response)
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
        let attempted_as = match whoami(params.http_client, params.registry, params.authorization).await {
            Ok(Some(username)) => username,
            Ok(None) => "an anonymous user".to_string(),
            Err(_) => "an unknown user".to_string(),
        };

        return Err(Error::AuthenticationError(
            format!("Invalid authentication (as {})", attempted_as)
        ));
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

async fn whoami(http_client: &HttpClient, registry: &str, authorization: Option<&str>) -> Result<Option<String>, Error> {
    let Some(authorization) = authorization else {
        return Ok(None);
    };

    let response
        = http_client
            .get(format!("{}/-/whoami", registry))?
            .header("authorization", Some(authorization))
            .send()
            .await?;

    if !response.status().is_success() {
        return Ok(None);
    }

    #[derive(Deserialize)]
    struct WhoamiResponse {
        username: String,
    }

    let body
        = response.text().await?;

    let data: WhoamiResponse
        = JsonDocument::hydrate_from_str(&body)?;

    Ok(Some(data.username))
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

        current_report().await.as_ref()
            .map(|report| report.info(formatted_notice.to_string()));
    }
}

async fn ask_for_otp(params: &NpmHttpParams<'_>, response: &Response) -> Result<String, Error> {
    if let Some(otp) = params.otp {
        return Ok(otp.to_owned());
    }

    if std::env::var("YARN_IS_TEST_ENV").is_ok() {
        return Ok(std::env::var("YARN_INJECT_NPM_2FA_TOKEN").unwrap_or_default());
    }

    render_otp_notice(&response).await;

    let otp = current_report().await.as_ref()
        .map(|report| report.prompt(PromptType::Input("One-time password".to_string())))
        .unwrap()
        .await;

    Ok(otp)
}
