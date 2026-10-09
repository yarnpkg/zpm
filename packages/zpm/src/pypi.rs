//! PyPI index client.
//!
//! Yarn only talks to indexes through the Simple Repository API: PEP 691
//! (JSON, preferred through content negotiation) with a fallback on the
//! PEP 503 HTML pages. Dependency metadata is read from the PEP 658 / 714
//! `.metadata` files when the index advertises them, and from the wheel's
//! `METADATA` (or the sdist's `PKG-INFO`) otherwise.

use std::{collections::BTreeMap, str::FromStr, sync::{Arc, LazyLock}};

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use regex::Regex;
use serde::Deserialize;
use tokio::sync::OnceCell;
use zpm_config::{Configuration, EcosystemFilter, PackageRule, SourceRule};
use zpm_primitives::{Ident, PypiVersion, canonicalize_pypi_name};
use zpm_utils::{FromFileString, Hash64, Path, ToFileString};

use crate::{
    error::Error,
    http::HttpClient,
    python_env::{PythonEnv, PythonTargets, supported_tags, wheel_priority},
};

pub const SIMPLE_JSON_ACCEPT: &str
    = "application/vnd.pypi.simple.v1+json, application/vnd.pypi.simple.v1+html;q=0.2, text/html;q=0.01";

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, Deserialize)]
pub struct PypiFile {
    pub filename: String,
    pub url: String,
    pub requires_python: Option<String>,
    pub yanked: bool,
    pub upload_time: Option<DateTime<Utc>>,
    pub has_metadata: bool,
    pub sha256: Option<String>,
}

impl PypiFile {
    pub fn is_wheel(&self) -> bool {
        self.filename.ends_with(".whl")
    }

    pub fn is_sdist(&self) -> bool {
        self.filename.ends_with(".tar.gz") || self.filename.ends_with(".zip") || self.filename.ends_with(".tgz")
    }

    pub fn matches_python(&self, targets: &PythonTargets) -> bool {
        let Some(requires_python) = self.requires_python.as_deref().filter(|value| !value.trim().is_empty()) else {
            return true;
        };

        let Ok(specifiers) = pep440_rs::VersionSpecifiers::from_str(requires_python) else {
            return true;
        };

        specifiers.contains(&targets.python.pep440())
    }

    pub fn url_without_fragment(&self) -> &str {
        self.url.split('#').next().unwrap()
    }
}

#[derive(Clone, Debug, Default, serde::Serialize, Deserialize)]
pub struct PypiProject {
    pub releases: BTreeMap<PypiVersion, Vec<PypiFile>>,
}

impl PypiProject {
    /// Versions sorted from the highest to the lowest according to PEP 440.
    pub fn sorted_versions(&self) -> Vec<(pep440_rs::Version, &PypiVersion)> {
        let mut versions
            = self.releases.keys()
                .filter_map(|version| pep440_rs::Version::from_str(version.as_str()).ok().map(|parsed| (parsed, version)))
                .collect::<Vec<_>>();

        versions.sort_by(|a, b| b.0.cmp(&a.0));
        versions
    }
}

// ---------------------------------------------------------------------------
// Configuration lookups
// ---------------------------------------------------------------------------

fn package_rule_matches(rule: &PackageRule, ident: &Ident) -> bool {
    rule.ecosystem_filter.value.map_or(true, |filter| filter == EcosystemFilter::Pypi)
        && rule.package_filter.value.as_ref().map_or(true, |filter| filter.check(ident))
}

fn source_rule_matches(rule: &SourceRule, registry: &str) -> bool {
    rule.ecosystem_filter.value.map_or(true, |filter| filter == EcosystemFilter::Pypi)
        && rule.registry_filter.value.as_ref().map_or(true, |filter| normalize_index_url(filter) == normalize_index_url(registry))
}

/// Normalizes an index URL so it ends with a slash; `https://pypi.org` is
/// accepted as an alias for its Simple API root.
pub fn normalize_index_url(url: &str) -> String {
    let trimmed
        = url.trim().trim_end_matches('/');

    if trimmed == "https://pypi.org" || trimmed == "http://pypi.org" {
        return "https://pypi.org/simple/".to_string();
    }

    format!("{}/", trimmed)
}

pub fn get_registry(config: &Configuration, ident: &Ident) -> String {
    let mut registry
        = config.settings.pypi_registry_server.value.as_str();

    for rule in &config.settings.package_rules {
        if package_rule_matches(rule, ident) {
            if let Some(value) = rule.pypi_registry_server.value.as_deref() {
                registry = value;
            }
        }
    }

    normalize_index_url(registry)
}

/// The credentials configured for an index (and package), as found in
/// `pypiAuthIdent` (`user:password`) or `pypiAuthToken`, with `sourceRules`
/// and `packageRules` applied.
pub enum PypiCredentials {
    Ident(String),
    Token(String),
}

pub fn get_credentials(config: &Configuration, registry: &str, ident: Option<&Ident>) -> Option<PypiCredentials> {
    let mut token
        = config.settings.pypi_auth_token.value.as_ref().map(|secret| secret.value.clone());
    let mut auth_ident
        = config.settings.pypi_auth_ident.value.as_ref().map(|secret| secret.value.clone());

    for rule in &config.settings.source_rules {
        if source_rule_matches(rule, registry) {
            if let Some(value) = &rule.pypi_auth_token.value {
                token = Some(value.value.clone());
            }

            if let Some(value) = &rule.pypi_auth_ident.value {
                auth_ident = Some(value.value.clone());
            }
        }
    }

    if let Some(ident) = ident {
        for rule in &config.settings.package_rules {
            if package_rule_matches(rule, ident) {
                if let Some(value) = &rule.pypi_auth_token.value {
                    token = Some(value.value.clone());
                }

                if let Some(value) = &rule.pypi_auth_ident.value {
                    auth_ident = Some(value.value.clone());
                }
            }
        }
    }

    if let Some(auth_ident) = auth_ident.filter(|value| !value.is_empty()) {
        return Some(PypiCredentials::Ident(auth_ident));
    }

    token.filter(|value| !value.is_empty())
        .map(PypiCredentials::Token)
}

pub fn get_authorization(config: &Configuration, registry: &str, ident: Option<&Ident>) -> Option<String> {
    match get_credentials(config, registry, ident)? {
        PypiCredentials::Ident(auth_ident) => Some(format!("Basic {}", base64::Engine::encode(&base64::engine::general_purpose::STANDARD, auth_ident.as_bytes()))),
        PypiCredentials::Token(token) => Some(format!("Bearer {}", token)),
    }
}

/// Authorization to use when downloading an artifact. Credentials are only
/// sent to the host that served the index page they come from.
pub fn get_artifact_authorization(config: &Configuration, ident: &Ident, artifact_url: &str) -> Option<String> {
    let registry
        = get_registry(config, ident);

    let registry_host
        = url::Url::parse(&registry).ok()?.host_str()?.to_string();
    let artifact_host
        = url::Url::parse(artifact_url).ok()?.host_str()?.to_string();

    if registry_host != artifact_host {
        return None;
    }

    get_authorization(config, &registry, Some(ident))
}

pub fn get_minimal_age_gate(config: &Configuration, registry: &str, ident: &Ident) -> std::time::Duration {
    let mut value
        = config.settings.pypi_minimal_age_gate.value;

    for rule in &config.settings.source_rules {
        if source_rule_matches(rule, registry) {
            if let Some(next) = rule.pypi_minimal_age_gate.value {
                value = next;
            }
        }
    }

    for rule in &config.settings.package_rules {
        if package_rule_matches(rule, ident) {
            if let Some(next) = rule.pypi_minimal_age_gate.value {
                value = next;
            }
        }
    }

    value
}

pub fn get_upload_cutoff(config: &Configuration, ident: &Ident) -> Option<DateTime<Utc>> {
    let registry
        = get_registry(config, ident);

    let gate
        = get_minimal_age_gate(config, &registry, ident);

    if gate.is_zero() {
        return None;
    }

    let gate
        = chrono::Duration::from_std(gate).ok()?;

    Some(Utc::now() - gate)
}

// ---------------------------------------------------------------------------
// Simple API
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct SimpleJsonProject {
    #[serde(default)]
    files: Vec<SimpleJsonFile>,
}

#[derive(Deserialize)]
struct SimpleJsonFile {
    filename: String,
    url: String,

    #[serde(default, rename = "requires-python")]
    requires_python: Option<String>,

    #[serde(default)]
    yanked: serde_json::Value,

    #[serde(default, rename = "upload-time")]
    upload_time: Option<String>,

    #[serde(default, rename = "core-metadata")]
    core_metadata: serde_json::Value,

    #[serde(default, rename = "dist-info-metadata")]
    dist_info_metadata: serde_json::Value,

    #[serde(default, rename = "data-dist-info-metadata")]
    data_dist_info_metadata: serde_json::Value,

    #[serde(default)]
    hashes: BTreeMap<String, String>,
}

fn json_truthy(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Null => false,
        serde_json::Value::Bool(value) => *value,
        serde_json::Value::String(value) => !value.is_empty(),
        _ => true,
    }
}

fn parse_upload_time(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value).ok()
        .map(|time| time.with_timezone(&Utc))
}

/// Extracts the version out of a distribution filename (`foo-1.0-py3-none-any.whl`
/// or `foo-1.0.tar.gz`), checking that the distribution name matches.
pub fn version_from_filename(ident: &Ident, filename: &str) -> Option<PypiVersion> {
    if let Some(stem) = filename.strip_suffix(".whl") {
        let mut parts
            = stem.split('-');

        let name
            = parts.next()?;
        let version
            = parts.next()?;

        if canonicalize_pypi_name(name) != ident.as_str() {
            return None;
        }

        return PypiVersion::from_file_string(version).ok();
    }

    let stem
        = filename.strip_suffix(".tar.gz")
            .or_else(|| filename.strip_suffix(".tgz"))
            .or_else(|| filename.strip_suffix(".zip"))?;

    stem.match_indices('-').find_map(|(offset, _)| {
        let (name, version)
            = (&stem[..offset], &stem[offset + 1..]);

        (canonicalize_pypi_name(name) == ident.as_str())
            .then(|| PypiVersion::from_file_string(version).ok())
            .flatten()
    })
}

fn parse_json_project(bytes: &[u8], base_url: &url::Url, ident: &Ident) -> Result<PypiProject, Error> {
    let project: SimpleJsonProject
        = serde_json::from_slice(bytes)
            .map_err(|err| Error::InvalidResolution(format!("Invalid PEP 691 response for {}: {}", ident.as_str(), err)))?;

    let mut releases
        = BTreeMap::<PypiVersion, Vec<PypiFile>>::new();

    for file in project.files {
        let Some(version) = version_from_filename(ident, &file.filename) else {
            continue;
        };

        let url
            = base_url.join(&file.url)?.to_string();

        let has_metadata
            = json_truthy(&file.core_metadata) || json_truthy(&file.dist_info_metadata) || json_truthy(&file.data_dist_info_metadata);

        releases.entry(version).or_default().push(PypiFile {
            filename: file.filename,
            url,
            requires_python: file.requires_python,
            yanked: json_truthy(&file.yanked),
            upload_time: file.upload_time.as_deref().and_then(parse_upload_time),
            has_metadata,
            sha256: file.hashes.get("sha256").cloned(),
        });
    }

    Ok(PypiProject {releases})
}

static ANCHOR_RE: LazyLock<Regex>
    = LazyLock::new(|| Regex::new(r#"(?is)<a\b([^>]*)>"#).unwrap());
static ATTRIBUTE_RE: LazyLock<Regex>
    = LazyLock::new(|| Regex::new(r#"(?is)([a-zA-Z0-9_-]+)(?:\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+)))?"#).unwrap());

fn decode_html(value: &str) -> String {
    value.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&#x27;", "'")
        .replace("&amp;", "&")
}

fn parse_html_project(html: &str, base_url: &url::Url, ident: &Ident) -> Result<PypiProject, Error> {
    let mut releases
        = BTreeMap::<PypiVersion, Vec<PypiFile>>::new();

    for anchor in ANCHOR_RE.captures_iter(html) {
        let attributes
            = anchor.get(1).map_or("", |value| value.as_str());

        let mut parsed
            = BTreeMap::<String, Option<String>>::new();

        for capture in ATTRIBUTE_RE.captures_iter(attributes) {
            let name
                = capture[1].to_ascii_lowercase();
            let value
                = capture.get(2).or(capture.get(3)).or(capture.get(4))
                    .map(|value| decode_html(value.as_str()));

            parsed.insert(name, value);
        }

        let Some(Some(href)) = parsed.get("href") else {
            continue;
        };

        let url
            = base_url.join(href)?;

        let Some(raw_filename) = url.path_segments().and_then(|mut segments| segments.next_back()) else {
            continue;
        };

        let filename
            = percent_decode(raw_filename);

        let Some(version) = version_from_filename(ident, &filename) else {
            continue;
        };

        let sha256
            = url.fragment()
                .and_then(|fragment| fragment.strip_prefix("sha256="))
                .map(|hash| hash.to_string());

        let has_metadata
            = parsed.get("data-core-metadata").or(parsed.get("data-dist-info-metadata"))
                .map_or(false, |value| value.as_deref() != Some("false"));

        releases.entry(version).or_default().push(PypiFile {
            filename,
            url: url.to_string(),
            requires_python: parsed.get("data-requires-python").cloned().flatten(),
            yanked: parsed.contains_key("data-yanked"),
            upload_time: None,
            has_metadata,
            sha256,
        });
    }

    Ok(PypiProject {releases})
}

fn percent_decode(value: &str) -> String {
    let bytes
        = value.as_bytes();

    let mut decoded
        = Vec::with_capacity(bytes.len());

    let mut index
        = 0;

    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(&value[index + 1..index + 3], 16) {
                decoded.push(byte);
                index += 3;
                continue;
            }
        }

        decoded.push(bytes[index]);
        index += 1;
    }

    String::from_utf8_lossy(&decoded).to_string()
}

type ProjectCell = Arc<OnceCell<Arc<PypiProject>>>;

static PROJECT_CACHE: LazyLock<DashMap<String, ProjectCell>>
    = LazyLock::new(DashMap::new);

fn project_cache_path(config: &Configuration, project_url: &str) -> Path {
    config.settings.global_folder.value
        .with_join_str("pypi-index")
        .with_join_str(format!("{}.json", Hash64::from_data(project_url.as_bytes()).to_file_string()))
}

fn read_cached_project(config: &Configuration, project_url: &str) -> Option<PypiProject> {
    let bytes
        = project_cache_path(config, project_url).fs_read().ok()?;

    serde_json::from_slice(&bytes).ok()
}

async fn fetch_project_from_network(config: &Configuration, http_client: &HttpClient, registry: &str, project_url: &str, ident: &Ident) -> Result<PypiProject, Error> {
    let authorization
        = get_authorization(config, registry, Some(ident));

    let request
        = http_client.get(project_url)?
            .header("accept", Some(SIMPLE_JSON_ACCEPT))
            .header("authorization", authorization.as_deref());

    let (response, bytes)
        = request.send_bytes().await?;

    let content_type
        = response.headers().get("content-type")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_ascii_lowercase();

    let base_url
        = url::Url::parse(response.url().as_str())?;

    let project = if content_type.contains("json") {
        parse_json_project(&bytes, &base_url, ident)?
    } else {
        parse_html_project(&String::from_utf8_lossy(&bytes), &base_url, ident)?
    };

    if let Ok(serialized) = serde_json::to_vec(&project) {
        let _ = project_cache_path(config, project_url).fs_create_parent()
            .and_then(|path| path.fs_write(&serialized));
    }

    Ok(project)
}

/// Fetches the list of files available for a project on the index it's
/// routed to. Results are memoized for the duration of the process, and
/// persisted on disk: when `known_version` is set (the version was locked)
/// and the persisted listing contains it, the network isn't queried at all,
/// as release files are immutable.
pub async fn fetch_project(config: &Configuration, http_client: &HttpClient, ident: &Ident, known_version: Option<&PypiVersion>) -> Result<Arc<PypiProject>, Error> {
    let registry
        = get_registry(config, ident);

    let project_url
        = format!("{}{}/", registry, ident.as_str());

    if let Some(version) = known_version {
        if let Some(cell) = PROJECT_CACHE.get(&project_url).map(|cell| cell.clone()) {
            if let Some(project) = cell.get() {
                return Ok(project.clone());
            }
        }

        if let Some(project) = read_cached_project(config, &project_url) {
            if project.releases.contains_key(version) {
                return Ok(Arc::new(project));
            }
        }
    }

    let cell
        = PROJECT_CACHE.entry(project_url.clone())
            .or_default()
            .clone();

    let project = cell.get_or_try_init(|| async {
        Ok::<_, Error>(Arc::new(fetch_project_from_network(config, http_client, &registry, &project_url, ident).await?))
    }).await?;

    Ok(project.clone())
}

// ---------------------------------------------------------------------------
// Core metadata
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CoreMetadata {
    pub name: Option<String>,
    pub version: Option<String>,
    pub requires_dist: Vec<String>,
    pub requires_python: Option<String>,
    pub provides_extra: Vec<String>,
}

pub fn parse_core_metadata(text: &str) -> CoreMetadata {
    let mut headers
        = Vec::<String>::new();

    for line in text.lines() {
        let line
            = line.trim_end_matches('\r');

        if line.is_empty() {
            break;
        }

        if line.starts_with([' ', '\t']) {
            if let Some(previous) = headers.last_mut() {
                previous.push(' ');
                previous.push_str(line.trim_start());
            }
        } else {
            headers.push(line.to_string());
        }
    }

    let mut metadata
        = CoreMetadata::default();

    for header in headers {
        let Some((key, value)) = header.split_once(':') else {
            continue;
        };

        let value
            = value.trim().to_string();

        match key.trim().to_ascii_lowercase().as_str() {
            "name" => metadata.name = Some(value),
            "version" => metadata.version = Some(value),
            "requires-dist" => metadata.requires_dist.push(value),
            "requires-python" => metadata.requires_python = Some(value),
            "provides-extra" => metadata.provides_extra.push(value),
            _ => {},
        }
    }

    metadata
}

pub fn metadata_from_wheel(bytes: &[u8]) -> Result<CoreMetadata, Error> {
    let entries
        = zpm_formats::zip::entries_from_zip(bytes)?;

    let entry
        = entries.iter()
            .filter(|entry| entry.name.as_str().ends_with(".dist-info/METADATA"))
            .min_by_key(|entry| entry.name.as_str().matches('/').count())
            .ok_or_else(|| Error::InvalidResolution("Wheel is missing its .dist-info/METADATA file".to_string()))?;

    Ok(parse_core_metadata(&String::from_utf8_lossy(&entry.data)))
}

pub fn metadata_from_sdist(bytes: &[u8], filename: &str) -> Result<CoreMetadata, Error> {
    let entries = if filename.ends_with(".zip") {
        zpm_formats::zip::entries_from_zip(bytes)?
            .into_iter()
            .map(|entry| (entry.name.as_str().to_string(), entry.data.to_vec()))
            .collect::<Vec<_>>()
    } else {
        let tar
            = zpm_formats::tar::unpack_tgz(bytes)?;

        zpm_formats::tar::entries_from_tar(&tar)?
            .into_iter()
            .map(|entry| (entry.name.as_str().to_string(), entry.data.to_vec()))
            .collect::<Vec<_>>()
    };

    let pkg_info
        = entries.iter()
            .filter(|(name, _)| name.ends_with("PKG-INFO") && name.matches('/').count() <= 1)
            .min_by_key(|(name, _)| name.matches('/').count())
            .ok_or_else(|| Error::InvalidResolution(format!("{} has no PKG-INFO file", filename)))?;

    Ok(parse_core_metadata(&String::from_utf8_lossy(&pkg_info.1)))
}

fn metadata_cache_path(config: &Configuration, url: &str) -> Path {
    config.settings.global_folder.value
        .with_join_str("pypi-metadata")
        .with_join_str(format!("{}.txt", Hash64::from_data(url.as_bytes()).to_file_string()))
}

type MetadataCell = Arc<OnceCell<Arc<CoreMetadata>>>;

static METADATA_CACHE: LazyLock<DashMap<String, MetadataCell>>
    = LazyLock::new(DashMap::new);

/// Retrieves the core metadata of a file. Release files are immutable, so
/// the metadata is persisted in the global folder keyed by the file URL.
pub async fn fetch_core_metadata(config: &Configuration, http_client: &HttpClient, ident: &Ident, file: &PypiFile) -> Result<Arc<CoreMetadata>, Error> {
    let cache_key
        = file.url_without_fragment().to_string();

    let cell
        = METADATA_CACHE.entry(cache_key.clone())
            .or_default()
            .clone();

    let metadata = cell.get_or_try_init(|| async {
        let cache_path
            = metadata_cache_path(config, &cache_key);

        if let Ok(text) = cache_path.fs_read_text() {
            return Ok::<_, Error>(Arc::new(parse_core_metadata(&text)));
        }

        let authorization
            = get_artifact_authorization(config, ident, &cache_key);

        let text = if file.has_metadata {
            let (_, bytes)
                = http_client.get(format!("{}.metadata", cache_key))?
                    .header("authorization", authorization.as_deref())
                    .send_bytes().await?;

            String::from_utf8_lossy(&bytes).to_string()
        } else {
            let (_, bytes)
                = http_client.get(&cache_key)?
                    .header("authorization", authorization.as_deref())
                    .send_bytes().await?;

            let metadata = if file.is_wheel() {
                metadata_from_wheel(&bytes)?
            } else {
                metadata_from_sdist(&bytes, &file.filename)?
            };

            serialize_core_metadata(&metadata)
        };

        let _ = cache_path.fs_create_parent()
            .and_then(|path| path.fs_write(text.as_bytes()));

        Ok(Arc::new(parse_core_metadata(&text)))
    }).await?;

    Ok(metadata.clone())
}

pub fn serialize_core_metadata(metadata: &CoreMetadata) -> String {
    let mut text
        = String::from("Metadata-Version: 2.1\n");

    if let Some(name) = &metadata.name {
        text.push_str(&format!("Name: {}\n", name));
    }

    if let Some(version) = &metadata.version {
        text.push_str(&format!("Version: {}\n", version));
    }

    if let Some(requires_python) = &metadata.requires_python {
        text.push_str(&format!("Requires-Python: {}\n", requires_python));
    }

    for extra in &metadata.provides_extra {
        text.push_str(&format!("Provides-Extra: {}\n", extra));
    }

    for requirement in &metadata.requires_dist {
        text.push_str(&format!("Requires-Dist: {}\n", requirement));
    }

    text
}

// ---------------------------------------------------------------------------
// Artifact selection
// ---------------------------------------------------------------------------

pub fn is_file_eligible(file: &PypiFile, targets: &PythonTargets, cutoff: Option<DateTime<Utc>>) -> bool {
    if file.yanked {
        return false;
    }

    is_file_installable(file, targets, cutoff)
}

/// Same as `is_file_eligible`, but tolerating yanked files. PEP 592: yanked
/// releases are never picked when choosing a version, but an installer
/// must still install one that's already pinned (a lockfile entry, `==`).
pub fn is_file_installable(file: &PypiFile, targets: &PythonTargets, cutoff: Option<DateTime<Utc>>) -> bool {

    if !file.matches_python(targets) {
        return false;
    }

    if let (Some(cutoff), Some(upload_time)) = (cutoff, file.upload_time) {
        if upload_time > cutoff {
            return false;
        }
    }

    file.is_wheel() || file.is_sdist()
}

/// Picks the best file to install in a given environment: the most
/// specific compatible wheel, or the sdist when no wheel matches.
pub fn select_file<'a>(files: &'a [PypiFile], env: &PythonEnv, targets: &PythonTargets, cutoff: Option<DateTime<Utc>>) -> Option<&'a PypiFile> {
    select_file_with(files, env, targets, |file| is_file_eligible(file, targets, cutoff))
}

/// Picks the artifact of a version that has already been chosen (from a
/// lockfile or by the solver): yanked files are acceptable there (PEP 592).
pub fn select_pinned_file<'a>(files: &'a [PypiFile], env: &PythonEnv, targets: &PythonTargets) -> Option<&'a PypiFile> {
    select_file(files, env, targets, None)
        .or_else(|| select_file_with(files, env, targets, |file| is_file_installable(file, targets, None)))
}

fn select_file_with<'a>(files: &'a [PypiFile], env: &PythonEnv, targets: &PythonTargets, is_eligible: impl Fn(&PypiFile) -> bool) -> Option<&'a PypiFile> {
    let supported
        = supported_tags(env, targets);

    let best_wheel
        = files.iter()
            .filter(|file| file.is_wheel() && is_eligible(file))
            .filter_map(|file| wheel_priority(&file.filename, &supported).map(|priority| (priority, file)))
            .min_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.filename.cmp(&b.1.filename)))
            .map(|(_, file)| file);

    best_wheel.or_else(|| {
        files.iter()
            .filter(|file| file.is_sdist() && is_eligible(file))
            .min_by_key(|file| if file.filename.ends_with(".tar.gz") { 0 } else { 1 })
    })
}

/// Picks the file to read dependency metadata from: prefers wheels whose
/// metadata is directly available, then the file we'd install here.
pub fn select_metadata_file<'a>(files: &'a [PypiFile], targets: &PythonTargets, cutoff: Option<DateTime<Utc>>) -> Option<&'a PypiFile> {
    let current
        = targets.current_env();

    let eligible
        = files.iter()
            .filter(|file| is_file_eligible(file, targets, cutoff))
            .collect::<Vec<_>>();

    eligible.iter()
        .find(|file| file.has_metadata && file.is_wheel())
        .copied()
        .or_else(|| select_file(files, &current, targets, cutoff))
        .or_else(|| eligible.iter().find(|file| file.is_wheel()).copied())
        .or_else(|| eligible.first().copied())
}

pub fn encode_path_segment(segment: &str) -> String {
    url::form_urlencoded::byte_serialize(segment.as_bytes())
        .collect::<String>()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_parsing() {
        let ident
            = Ident::new("acme-private");
        let html
            = r#"<a rel="internal" href="https://pypi.example.com/acme/-/ver_27Zuda/acme_private-1.2.2rc1-py3-none-any.whl#sha256=46" data-requires-python="&gt;=3.8,&lt;4.0">acme_private-1.2.2rc1-py3-none-any.whl</a><br/>
<a href="../../packages/acme-private-1.2.2rc1.tar.gz" data-yanked>x</a>"#;

        let project
            = parse_html_project(html, &url::Url::parse("https://pypi.example.com/acme/acme-private/").unwrap(), &ident).unwrap();

        let files
            = &project.releases[&PypiVersion::from_file_string("1.2.2rc1").unwrap()];

        assert_eq!(files.len(), 2);
        assert_eq!(files[0].requires_python.as_deref(), Some(">=3.8,<4.0"));
        assert_eq!(files[0].sha256.as_deref(), Some("46"));
        assert!(!files[0].yanked);
        assert!(files[1].yanked);
        assert_eq!(files[1].url, "https://pypi.example.com/packages/acme-private-1.2.2rc1.tar.gz");
    }

    #[test]
    fn metadata_parsing() {
        let metadata = parse_core_metadata("Metadata-Version: 2.1\nName: foo\nVersion: 1.0\nRequires-Dist: bar>=1;\n  extra == 'x'\nRequires-Dist: baz\n\nRequires-Dist: body\n");

        assert_eq!(metadata.name.as_deref(), Some("foo"));
        assert_eq!(metadata.requires_dist, vec!["bar>=1; extra == 'x'", "baz"]);
    }

    #[test]
    fn index_normalization() {
        assert_eq!(normalize_index_url("https://pypi.org"), "https://pypi.org/simple/");
        assert_eq!(normalize_index_url("https://pypi.example.com/acme"), "https://pypi.example.com/acme/");
    }
}
