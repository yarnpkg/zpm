//! Sparse registry index client.
//!
//! A sparse index serves one file per crate, each line of which describes
//! a published version: its dependencies, its features, and the checksum of
//! its `.crate` file. That's all the resolver needs, so nothing is
//! downloaded besides those files until the crates get fetched.
//!
//! <https://doc.rust-lang.org/cargo/reference/registry-index.html>

use std::{collections::BTreeMap, sync::{Arc, LazyLock}};

use dashmap::DashMap;
use reqwest::StatusCode;
use serde::Deserialize;
use tokio::sync::OnceCell;
use zpm_primitives::{CargoRequirement, CargoSemver};
use zpm_utils::{FromFileString, Hash64, Path, ToFileString};

use crate::error::Error;

use super::{CargoContext, CrateSource, Dependency, DependencyKind, DependencySource, Summary, add_implicit_features};

#[derive(Debug, Deserialize)]
struct IndexDependency {
    name: String,
    req: String,

    #[serde(default)]
    features: Vec<String>,

    #[serde(default)]
    optional: bool,

    #[serde(default = "default_true")]
    default_features: bool,

    #[serde(default)]
    target: Option<String>,

    #[serde(default)]
    kind: Option<String>,

    #[serde(default)]
    package: Option<String>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize)]
struct IndexVersion {
    name: String,
    vers: String,

    #[serde(default)]
    deps: Vec<IndexDependency>,

    cksum: String,

    #[serde(default)]
    features: BTreeMap<String, Vec<String>>,

    /// Features using the `dep:` and `?/` syntaxes are listed apart, so
    /// that the Cargo versions predating them ignore those releases
    #[serde(default)]
    features2: Option<BTreeMap<String, Vec<String>>>,

    #[serde(default)]
    yanked: bool,

    #[serde(default)]
    links: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct RegistryConfig {
    pub dl: String,
}

impl RegistryConfig {
    pub fn download_url(&self, name: &str, version: &str, checksum: &str) -> String {
        const MARKERS: [&str; 5]
            = ["{crate}", "{version}", "{prefix}", "{lowerprefix}", "{sha256-checksum}"];

        if !MARKERS.iter().any(|marker| self.dl.contains(marker)) {
            return format!("{}/{}/{}/download", self.dl.trim_end_matches('/'), name, version);
        }

        let prefix
            = index_prefix(name);

        self.dl
            .replace("{crate}", name)
            .replace("{version}", version)
            .replace("{lowerprefix}", &prefix.to_ascii_lowercase())
            .replace("{prefix}", &prefix)
            .replace("{sha256-checksum}", checksum)
    }
}

/// Every version of a crate the index lists, lowest first.
#[derive(Debug, Default)]
pub struct CrateVersions {
    pub versions: Vec<Arc<Summary>>,
}

impl CrateVersions {
    pub fn find(&self, version: &CargoSemver) -> Option<&Arc<Summary>> {
        self.versions.iter().find(|summary| &summary.version == version)
    }
}

/// `sparse+https://index.crates.io/` is how Cargo's own configuration
/// names sparse indexes.
pub fn normalize_index_url(url: &str) -> String {
    let url
        = url.trim().strip_prefix("sparse+").unwrap_or(url.trim());

    format!("{}/", url.trim_end_matches('/'))
}

/// The folder a crate is filed under.
fn index_prefix(name: &str) -> String {
    match name.len() {
        1 => "1".to_string(),
        2 => "2".to_string(),
        3 => format!("3/{}", &name[..1]),
        _ => format!("{}/{}", &name[..2], &name[2..4]),
    }
}

pub fn index_path(name: &str) -> String {
    let name
        = name.to_ascii_lowercase();

    format!("{}/{}", index_prefix(&name), name)
}

fn parse_kind(kind: Option<&str>) -> DependencyKind {
    match kind {
        Some("build") => DependencyKind::Build,
        Some("dev") => DependencyKind::Development,
        _ => DependencyKind::Normal,
    }
}

fn parse_version(line: &str) -> Option<Summary> {
    let entry: IndexVersion
        = serde_json::from_str(line).ok()?;

    let version
        = CargoSemver::parse(&entry.vers)?;

    // Like Cargo, entries we can't make sense of (very old releases with
    // requirements that aren't valid anymore) are ignored
    let dependencies = entry.deps.into_iter().map(|dependency| {
        let requirement
            = CargoRequirement::from_file_string(&dependency.req).ok()?;

        Some(Dependency {
            package: dependency.package.unwrap_or_else(|| dependency.name.clone()),
            name_in_toml: dependency.name,
            req: requirement.to_req(),
            requirement,
            features: dependency.features,
            default_features: dependency.default_features,
            optional: dependency.optional,
            kind: parse_kind(dependency.kind.as_deref()),
            target: dependency.target,
            source: DependencySource::Registry,
        })
    }).collect::<Option<Vec<_>>>()?;

    let mut features
        = entry.features;

    for (feature, values) in entry.features2.unwrap_or_default() {
        features.entry(feature).or_default().extend(values);
    }

    add_implicit_features(&mut features, &dependencies);

    Some(Summary {
        name: entry.name,
        version,
        source: CrateSource::Registry,
        dependencies,
        features,
        checksum: Some(entry.cksum),
        yanked: entry.yanked,
        links: entry.links,
    })
}

pub fn parse_index_file(text: &str) -> CrateVersions {
    let mut versions = text.lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(parse_version)
        .map(Arc::new)
        .collect::<Vec<_>>();

    versions.sort_by(|a, b| a.version.cmp(&b.version));

    CrateVersions {versions}
}

type CrateCell = Arc<OnceCell<Arc<CrateVersions>>>;

static CRATE_CACHE: LazyLock<DashMap<String, CrateCell>>
    = LazyLock::new(DashMap::new);

static CONFIG_CACHE: LazyLock<DashMap<String, Arc<OnceCell<RegistryConfig>>>>
    = LazyLock::new(DashMap::new);

fn index_cache_dir(context: &CargoContext) -> Path {
    context.cache_dir
        .with_join_str("index")
        .with_join_str(Hash64::from_data(context.index_url.as_bytes()).mini())
}

async fn fetch_index_file(context: &CargoContext, path: &str, what: &str) -> Result<String, Error> {
    let url
        = format!("{}{}", context.index_url, path);

    let response
        = context.http_client.get(&url)?.send_bytes().await;

    match response {
        Ok((_, bytes)) => Ok(String::from_utf8_lossy(&bytes).to_string()),

        Err(err) if matches!(err.status(), Some(StatusCode::NOT_FOUND | StatusCode::GONE)) => {
            Err(Error::CargoError(format!("{} doesn't exist on {}", what, context.index_url)))
        },

        Err(err) => Err(Error::RemoteRegistryError(Arc::new(err))),
    }
}

/// Fetches the versions of a crate. Results are memoized for the duration
/// of the process and persisted on the disk; when `known_version` is set
/// and the persisted listing has it the network isn't queried at all, as
/// published versions never change (they can only get yanked).
pub async fn fetch_crate(context: &CargoContext, name: &str, known_version: Option<&CargoSemver>) -> Result<Arc<CrateVersions>, Error> {
    let path
        = index_path(name);

    let memo_key
        = format!("{}{}", context.index_url, path);

    let cache_path
        = index_cache_dir(context).with_join_str(&path);

    if let Some(version) = known_version {
        if let Some(versions) = CRATE_CACHE.get(&memo_key).and_then(|cell| cell.get().cloned()) {
            if versions.find(version).is_some() {
                return Ok(versions);
            }
        }

        if let Ok(text) = cache_path.fs_read_text() {
            let versions
                = parse_index_file(&text);

            if versions.find(version).is_some() {
                return Ok(Arc::new(versions));
            }
        }
    }

    let cell
        = CRATE_CACHE.entry(memo_key).or_default().clone();

    let versions = cell.get_or_try_init(|| async {
        let text
            = fetch_index_file(context, &path, &format!("The crate {}", name)).await?;

        let _ = cache_path.fs_create_parent()
            .and_then(|path| path.fs_write_text(&text));

        Ok::<_, Error>(Arc::new(parse_index_file(&text)))
    }).await?;

    Ok(versions.clone())
}

/// The `config.json` of the index, which tells where crates are served.
pub async fn fetch_config(context: &CargoContext) -> Result<RegistryConfig, Error> {
    let cell
        = CONFIG_CACHE.entry(context.index_url.clone()).or_default().clone();

    let config = cell.get_or_try_init(|| async {
        let cache_path
            = index_cache_dir(context).with_join_str("config.json");

        let cached = cache_path.fs_read_text().ok()
            .and_then(|text| serde_json::from_str::<RegistryConfig>(&text).ok());

        if let Some(config) = cached {
            return Ok(config);
        }

        let text
            = fetch_index_file(context, "config.json", "The registry configuration").await?;

        let config = serde_json::from_str::<RegistryConfig>(&text)
            .map_err(|err| Error::CargoError(format!("Invalid registry configuration on {}: {}", context.index_url, err)))?;

        let _ = cache_path.fs_create_parent()
            .and_then(|path| path.fs_write_text(&text));

        Ok::<_, Error>(config)
    }).await?;

    Ok(config.clone())
}

/// The summary of one version of a registry crate.
pub async fn fetch_version(context: &CargoContext, name: &str, version: &CargoSemver) -> Result<Arc<Summary>, Error> {
    let versions
        = fetch_crate(context, name, Some(version)).await?;

    versions.find(version).cloned()
        .ok_or_else(|| Error::CargoError(format!("Version {} of the crate {} isn't listed on {}", version, name, context.index_url.to_file_string())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_index_paths() {
        assert_eq!(index_path("a"), "1/a");
        assert_eq!(index_path("ab"), "2/ab");
        assert_eq!(index_path("abc"), "3/a/abc");
        assert_eq!(index_path("serde"), "se/rd/serde");
        assert_eq!(index_path("Inflector"), "in/fl/inflector");
    }

    #[test]
    fn test_index_urls() {
        assert_eq!(normalize_index_url("sparse+https://index.crates.io"), "https://index.crates.io/");
        assert_eq!(normalize_index_url("https://example.com/index/"), "https://example.com/index/");
    }

    #[test]
    fn test_download_urls() {
        let plain
            = RegistryConfig {dl: "https://static.crates.io/crates".to_string()};

        assert_eq!(plain.download_url("serde", "1.0.228", "abc"), "https://static.crates.io/crates/serde/1.0.228/download");

        let template
            = RegistryConfig {dl: "https://example.com/{prefix}/{crate}/{crate}-{version}.crate".to_string()};

        assert_eq!(template.download_url("serde", "1.0.228", "abc"), "https://example.com/se/rd/serde/serde-1.0.228.crate");
    }

    #[test]
    fn test_index_file() {
        let text = [
            r#"{"name":"demo","vers":"0.2.0","deps":[{"name":"renamed","req":"^1","features":["a"],"optional":true,"default_features":false,"target":"cfg(unix)","kind":"normal","package":"real"},{"name":"gated","req":"^2","features":[],"optional":true,"default_features":true,"target":null,"kind":"normal"}],"cksum":"00","features":{"default":["std"],"std":[]},"features2":{"full":["dep:gated","renamed?/b"]},"yanked":false,"links":"demo","v":2}"#,
            r#"{"name":"demo","vers":"0.1.0","deps":[{"name":"bad","req":"not a requirement","features":[],"optional":false,"default_features":true,"target":null,"kind":"normal"}],"cksum":"11","features":{},"yanked":false}"#,
            r#"{"name":"demo","vers":"0.1.1","deps":[{"name":"dev","req":"*","features":[],"optional":false,"default_features":true,"target":null,"kind":"dev"}],"cksum":"22","features":{},"yanked":true}"#,
        ].join("\n");

        let versions
            = parse_index_file(&text);

        // The entry with an invalid requirement is dropped; the others are sorted
        assert_eq!(versions.versions.iter().map(|summary| summary.version.to_string()).collect::<Vec<_>>(), vec!["0.1.1", "0.2.0"]);

        let yanked
            = &versions.versions[0];

        assert!(yanked.yanked);
        assert_eq!(yanked.dependencies[0].kind, DependencyKind::Development);

        let latest
            = &versions.versions[1];

        assert_eq!(latest.checksum.as_deref(), Some("00"));
        assert_eq!(latest.links.as_deref(), Some("demo"));

        assert_eq!(latest.dependencies[0].name_in_toml, "renamed");
        assert_eq!(latest.dependencies[0].package, "real");
        assert_eq!(latest.dependencies[0].target.as_deref(), Some("cfg(unix)"));
        assert!(!latest.dependencies[0].default_features);

        // `features2` is merged, and only `renamed` gets an implicit
        // feature, as `gated` is referred to through `dep:`
        assert_eq!(latest.features.keys().map(|key| key.as_str()).collect::<Vec<_>>(), vec!["default", "full", "renamed", "std"]);
    }
}
