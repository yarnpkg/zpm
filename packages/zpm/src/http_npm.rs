use std::collections::HashSet;
use std::sync::{Arc, LazyLock, Mutex};

use bytes::Bytes;
use dashmap::DashMap;
use regex::{Captures, Regex};
use reqwest::{Response, Url};
use serde::Deserialize;
use sha2::{Sha256, Digest};
use tokio::sync::OnceCell;
use tokio::task::JoinHandle;
use zpm_config::{Configuration, EcosystemFilter, NpmRegistry, NpmScope, PackageRule, SourceRule};
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

fn normalize_registry_url(registry: &str) -> &str {
    registry.trim_end_matches('/')
}

fn ecosystem_matches(filter: Option<EcosystemFilter>, ecosystem: EcosystemFilter) -> bool {
    filter.map_or(true, |filter| filter == ecosystem)
}

fn package_rule_matches(rule: &PackageRule, ecosystem: EcosystemFilter, ident: Option<&Ident>) -> bool {
    ecosystem_matches(rule.ecosystem_filter.value, ecosystem)
        && rule.package_filter.value.as_ref().map_or(true, |filter| {
            ident.map_or(false, |ident| filter.check(ident))
        })
}

fn source_rule_matches(rule: &SourceRule, ecosystem: EcosystemFilter, registry: &str) -> bool {
    ecosystem_matches(rule.ecosystem_filter.value, ecosystem)
        && rule.registry_filter.value.as_ref().map_or(true, |filter| {
            normalize_registry_url(filter) == normalize_registry_url(registry)
        })
}

fn legacy_scope_settings<'a>(config: &'a Configuration, ident: Option<&Ident>) -> Option<&'a NpmScope> {
    let scope = ident?.scope()?;
    config.settings.npm_scopes.get(scope.strip_prefix('@').unwrap_or(scope))
}

fn legacy_registry_settings<'a>(config: &'a Configuration, registry: &str) -> Option<&'a NpmRegistry> {
    if let Some(registry_settings) = config.settings.npm_registries.get(registry) {
        return Some(registry_settings);
    }

    let registry_with_slash = format!("{}/", registry);
    if let Some(registry_settings) = config.settings.npm_registries.get(&registry_with_slash) {
        return Some(registry_settings);
    }

    let normalized_registry
        = normalize_registry_url(registry);

    config.settings.npm_registries.iter()
        .find_map(|(candidate, settings)| {
            (normalize_registry_url(candidate) == normalized_registry).then_some(settings)
        })
}

fn get_registry_raw<'a>(config: &'a Configuration, ident: Option<&Ident>, publish: bool) -> Result<&'a str, Error> {
    let mut registry
        = config.settings.npm_registry_server.value.as_str();

    if publish {
        if let Some(value) = config.settings.npm_publish_registry.value.as_deref() {
            registry = value;
        }
    }

    if let Some(scope_settings) = legacy_scope_settings(config, ident) {
        if let Some(value) = scope_settings.npm_registry_server.value.as_deref() {
            registry = value;
        }
    }

    if publish {
        if let Some(scope_settings) = legacy_scope_settings(config, ident) {
            if let Some(value) = scope_settings.npm_publish_registry.value.as_deref() {
                registry = value;
            }
        }
    }

    for rule in &config.settings.package_rules {
        if package_rule_matches(rule, EcosystemFilter::Npm, ident) {
            if let Some(value) = rule.npm_registry_server.value.as_deref() {
                registry = value;
            }
        }
    }

    if publish {
        for rule in &config.settings.package_rules {
            if package_rule_matches(rule, EcosystemFilter::Npm, ident) {
                if let Some(value) = rule.npm_publish_registry.value.as_deref() {
                    registry = value;
                }
            }
        }
    }

    Ok(registry)
}

pub fn get_registry_for_ident<'a>(config: &'a Configuration, ident: Option<&Ident>, publish: bool) -> Result<&'a str, Error> {
    let registry
        = get_registry_raw(config, ident, publish)?;

    Ok(normalize_registry_url(registry))
}

pub fn get_registry<'a>(config: &'a Configuration, scope: Option<&str>, publish: bool) -> Result<&'a str, Error> {
    let scoped_ident
        = scope.map(|scope| Ident::new(format!("@{}/*", scope.strip_prefix('@').unwrap_or(scope))));

    get_registry_for_ident(config, scoped_ident.as_ref(), publish)
}

fn get_npm_always_auth(config: &Configuration, registry: &str, ident: Option<&Ident>) -> Option<bool> {
    let mut value
        = None;

    if let Some(registry_settings) = legacy_registry_settings(config, registry) {
        if let Some(next) = registry_settings.npm_always_auth.value {
            value = Some(next);
        }
    }

    if let Some(scope_settings) = legacy_scope_settings(config, ident) {
        if let Some(next) = scope_settings.npm_always_auth.value {
            value = Some(next);
        }
    }

    for rule in &config.settings.source_rules {
        if source_rule_matches(rule, EcosystemFilter::Npm, registry) {
            if let Some(next) = rule.npm_always_auth.value {
                value = Some(next);
            }
        }
    }

    for rule in &config.settings.package_rules {
        if package_rule_matches(rule, EcosystemFilter::Npm, ident) {
            if let Some(next) = rule.npm_always_auth.value {
                value = Some(next);
            }
        }
    }

    value
}

fn get_npm_auth_token<'a>(config: &'a Configuration, registry: &str, ident: Option<&Ident>) -> Option<&'a zpm_utils::Secret<String>> {
    let mut value
        = None;

    if let Some(registry_settings) = legacy_registry_settings(config, registry) {
        if let Some(next) = registry_settings.npm_auth_token.value.as_ref() {
            value = Some(next);
        }
    }

    if let Some(scope_settings) = legacy_scope_settings(config, ident) {
        if let Some(next) = scope_settings.npm_auth_token.value.as_ref() {
            value = Some(next);
        }
    }

    for rule in &config.settings.source_rules {
        if source_rule_matches(rule, EcosystemFilter::Npm, registry) {
            if let Some(next) = rule.npm_auth_token.value.as_ref() {
                value = Some(next);
            }
        }
    }

    for rule in &config.settings.package_rules {
        if package_rule_matches(rule, EcosystemFilter::Npm, ident) {
            if let Some(next) = rule.npm_auth_token.value.as_ref() {
                value = Some(next);
            }
        }
    }

    value
}

fn get_npm_auth_ident<'a>(config: &'a Configuration, registry: &str, ident: Option<&Ident>) -> Option<&'a String> {
    let mut value
        = None;

    if let Some(registry_settings) = legacy_registry_settings(config, registry) {
        if let Some(next) = registry_settings.npm_auth_ident.value.as_ref() {
            value = Some(next);
        }
    }

    if let Some(scope_settings) = legacy_scope_settings(config, ident) {
        if let Some(next) = scope_settings.npm_auth_ident.value.as_ref() {
            value = Some(next);
        }
    }

    for rule in &config.settings.source_rules {
        if source_rule_matches(rule, EcosystemFilter::Npm, registry) {
            if let Some(next) = rule.npm_auth_ident.value.as_ref() {
                value = Some(next);
            }
        }
    }

    for rule in &config.settings.package_rules {
        if package_rule_matches(rule, EcosystemFilter::Npm, ident) {
            if let Some(next) = rule.npm_auth_ident.value.as_ref() {
                value = Some(next);
            }
        }
    }

    value
}

pub fn get_minimal_age_gate(config: &Configuration, registry: &str, ident: &Ident) -> std::time::Duration {
    let mut value
        = config.settings.npm_minimal_age_gate.value;

    for rule in &config.settings.source_rules {
        if source_rule_matches(rule, EcosystemFilter::Npm, registry) {
            if let Some(next) = rule.npm_minimal_age_gate.value {
                value = next;
            }
        }
    }

    for rule in &config.settings.package_rules {
        if package_rule_matches(rule, EcosystemFilter::Npm, Some(ident)) {
            if let Some(next) = rule.npm_minimal_age_gate.value {
                value = next;
            }
        }
    }

    value
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
            get_npm_always_auth(options.configuration, options.registry, options.ident)
                .unwrap_or(options.configuration.settings.npm_always_auth.value)
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
            get_npm_always_auth(options.configuration, options.registry, options.ident)
                .unwrap_or(options.configuration.settings.npm_always_auth.value)
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
        = get_npm_auth_token(options.configuration, options.registry, options.ident)
            .or_else(|| options.configuration.settings.npm_auth_token.value.as_ref());

    if let Some(auth_token) = auth_token {
        return Ok(Some(format!("Bearer {}", auth_token.value)));
    }

    let auth_ident
        = get_npm_auth_ident(options.configuration, options.registry, options.ident)
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

/// Fetch a response without retaining its body in the process-wide HTTP
/// cache. Package archives are already deduplicated by the install fetch map
/// and can be released as soon as they have been converted into cache zips.
pub async fn get_uncached(params: &NpmHttpParams<'_>) -> Result<Bytes, Error> {
    let url
        = format!("{}{}", params.registry, params.path);

    let response = params.http_client.get(&url)?
        .header("authorization", params.authorization)
        .enable_status_check(false)
        .send().await?;

    if params.authorization.is_some() {
        handle_invalid_authentication_error(params, &response).await?;
    }

    Ok(response.error_for_status()?.bytes().await?)
}

const CACHED_VERSION_FIELDS: &[&str] = &[
    "bin",
    "cpu",
    "dependenciesMeta",
    "dependencies",
    "dist",
    "libc",
    "name",
    "optionalDependencies",
    "os",
    "peerDependenciesMeta",
    "peerDependencies",
    "scripts",
    "version",
];

const CACHED_TOP_LEVEL_FIELDS: &[&str] = &[
    "dist-tags",
    "time",
    "versions",
];

static METADATA_CACHE_KEY: LazyLock<String> = LazyLock::new(|| {
    let mut hasher
        = Sha256::new();

    for field in CACHED_TOP_LEVEL_FIELDS {
        hasher.update(field.as_bytes());
    }

    for field in CACHED_VERSION_FIELDS {
        hasher.update(field.as_bytes());
    }

    hex::encode(&hasher.finalize()[..3])
});

static METADATA_CACHE: LazyLock<DashMap<String, Arc<OnceCell<Bytes>>>>
    = LazyLock::new(DashMap::new);

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
    /// Offloads the on-disk metadata cache write to a `spawn_blocking`
    /// task; the owner must `drain` before the runtime exits. `None`
    /// falls back to an inline write (slower under parallel fetches,
    /// but safe for one-shot subcommands with no drain point).
    pub background_writes: Option<&'a BackgroundWrites>,
}

/// Tracks `spawn_blocking` cache-write tasks so the install can await
/// them before returning. Without an explicit join, `tokio::main`
/// drops the runtime out from under the writes when the outer future
/// resolves.
#[derive(Default)]
pub struct BackgroundWrites {
    handles: Mutex<Vec<JoinHandle<()>>>,
}

impl BackgroundWrites {
    pub fn new() -> Self {
        Self::default()
    }

    fn track<F>(&self, f: F)
    where
        F: FnOnce() + Send + 'static,
    {
        let handle = tokio::task::spawn_blocking(f);
        self.handles.lock().unwrap().push(handle);
    }

    pub async fn drain(&self) {
        let handles = std::mem::take(&mut *self.handles.lock().unwrap());
        for handle in handles {
            let _ = handle.await;
        }
    }
}

pub async fn get_package_metadata(params: &GetPackageMetadataParams<'_>) -> Result<Bytes, Error> {
    let cache_key
        = params.ident.to_string();

    let cell
        = METADATA_CACHE
            .entry(cache_key)
            .or_default()
            .clone();

    let result = cell.get_or_try_init(|| async {
        fetch_metadata_with_disk_cache(params).await
    }).await?;

    Ok(result.clone())
}

pub fn ensure_metadata_cache_dir(global_folder: &Path) {
    let cache_dir = global_folder
        .with_join_str("metadata/npm")
        .with_join_str(&*METADATA_CACHE_KEY);

    let _ = cache_dir.fs_create_dir_all();
}

fn get_cache_file_path(global_folder: &Path, url: &str) -> Path {
    let mut hasher = Sha256::new();
    hasher.update(url.as_bytes());
    let url_hash = hex::encode(hasher.finalize());

    global_folder
        .with_join_str("metadata/npm")
        .with_join_str(&*METADATA_CACHE_KEY)
        .with_join_str(&format!("{}.bin", url_hash))
}

fn trim_metadata(raw: &[u8]) -> Vec<u8> {
    let Ok(mut metadata) = serde_json::from_slice::<serde_json::Value>(raw) else {
        return raw.to_vec();
    };

    let top_level_fields: HashSet<_>
        = CACHED_TOP_LEVEL_FIELDS.iter()
            .copied()
            .collect();

    let version_fields: HashSet<_>
        = CACHED_VERSION_FIELDS.iter()
            .copied()
            .collect();

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
    let entry = CachedMetadataDisk {
        metadata: trim_metadata(metadata),
        etag,
        last_modified,
    };

    let Ok(encoded) = rkyv::to_bytes::<rkyv::rancor::BoxedError>(&entry) else {
        return;
    };

    let _ = cache_file.fs_create_parent();

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
        = get_cache_file_path(params.global_folder, &url);

    let cached = if !params.refresh_lockfile {
        cache_file.fs_read_prealloc().ok()
            .and_then(|data| rkyv::from_bytes::<CachedMetadataDisk, rkyv::rancor::BoxedError>(&data).ok())
    } else {
        None
    };

    if let Some(ref cached) = cached {
        let url_obj
            = Url::parse(&url)?;

        if !params.http_client.config.is_network_enabled(&url_obj) {
            return Ok(Bytes::from(cached.metadata.clone()));
        }
    }

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

    let fresh_body
        = response.error_for_status()?.bytes().await?;

    // Keep stale version entries the fresh response omits so
    // resolution still works when a published version is later
    // removed or hidden behind an allow-list.
    let merged_body = match &cached {
        Some(cached) => Bytes::from(merge_versions_into_fresh(&cached.metadata, &fresh_body)),
        None => fresh_body,
    };

    let body_for_disk
        = merged_body.clone();
    let cache_file_for_disk
        = cache_file.clone();

    // Offload the trim/encode/write when the caller passes a tracker;
    // otherwise fall back to an inline write (safe for one-shot
    // subcommands with no drain point).
    if let Some(background_writes) = params.background_writes {
        background_writes.track(move || {
            write_cache_to_disk(&cache_file_for_disk, &body_for_disk, etag, last_modified);
        });
    } else {
        write_cache_to_disk(&cache_file_for_disk, &body_for_disk, etag, last_modified);
    }

    Ok(merged_body)
}

/// Reintroduces stale `versions` entries the fresh response dropped.
/// Fresh always wins on conflicts; only the version-map is merged.
fn merge_versions_into_fresh(stale: &[u8], fresh: &[u8]) -> Vec<u8> {
    let Ok(mut fresh_json) = serde_json::from_slice::<serde_json::Value>(fresh) else {
        return fresh.to_vec();
    };

    let Ok(stale_json) = serde_json::from_slice::<serde_json::Value>(stale) else {
        return fresh.to_vec();
    };

    let Some(stale_versions) = stale_json.get("versions").and_then(|v| v.as_object()) else {
        return fresh.to_vec();
    };

    // Fast-path: skip re-serialising hundreds of KBs of metadata when
    // every stale version is already in the fresh response.
    if let Some(fresh_versions) = fresh_json.get("versions").and_then(|v| v.as_object()) {
        if stale_versions.keys().all(|k| fresh_versions.contains_key(k)) {
            return fresh.to_vec();
        }
    }

    let Some(fresh_obj) = fresh_json.as_object_mut() else {
        return fresh.to_vec();
    };

    let fresh_versions_entry = fresh_obj
        .entry("versions".to_string())
        .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));

    if let Some(fresh_versions) = fresh_versions_entry.as_object_mut() {
        for (version, value) in stale_versions {
            fresh_versions.entry(version.clone()).or_insert_with(|| value.clone());
        }
    }

    serde_json::to_vec(&fresh_json).unwrap_or_else(|_| fresh.to_vec())
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

        crate::report::if_active_async(|report| {
            report.info(formatted_notice.to_string());
        }).await;
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

    let report_guard
        = current_report().await;

    let Some(report) = report_guard.as_ref() else {
        return Err(Error::AuthenticationError(
            "One-time password required; rerun this command with --otp <code>".to_string()
        ));
    };

    let otp = report
        .prompt(PromptType::Input("One-time password".to_string()))
        .await;

    Ok(otp)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body_with_versions(keys: &[&str]) -> Vec<u8> {
        let map: serde_json::Map<String, serde_json::Value> = keys.iter()
            .map(|k| (k.to_string(), serde_json::json!({"dist": {"tarball": "https://example/x.tgz"}})))
            .collect();
        serde_json::to_vec(&serde_json::json!({"name": "pkg", "versions": serde_json::Value::Object(map)})).unwrap()
    }

    #[test]
    fn merge_returns_fresh_unchanged_when_stale_is_a_subset() {
        let stale = body_with_versions(&["1.0.0", "1.1.0"]);
        let fresh = body_with_versions(&["1.0.0", "1.1.0", "2.0.0"]);
        // The fast-path should hand back `fresh` byte-for-byte instead
        // of going through a JSON round-trip that may reorder keys.
        assert_eq!(merge_versions_into_fresh(&stale, &fresh), fresh);
    }

    #[test]
    fn merge_layers_in_missing_stale_versions() {
        let stale = body_with_versions(&["1.0.0", "0.9.0"]);
        let fresh = body_with_versions(&["1.0.0", "2.0.0"]);
        let merged = merge_versions_into_fresh(&stale, &fresh);
        let parsed: serde_json::Value = serde_json::from_slice(&merged).unwrap();
        let versions = parsed.get("versions").unwrap().as_object().unwrap();
        // 0.9.0 from stale should now be present alongside the fresh
        // entries; 2.0.0 from fresh must still be there.
        assert!(versions.contains_key("0.9.0"));
        assert!(versions.contains_key("2.0.0"));
    }
}
