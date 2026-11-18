//! Rust support.
//!
//! Crates are resolved by islands using the `cargo` linker. Everything
//! Cargo would do during `cargo generate-lockfile` and `cargo vendor` is
//! implemented here, so that installing doesn't need a Rust toolchain:
//!
//! - `manifest` reads `Cargo.toml` files (workspaces, inheritance, targets);
//! - `index` talks to the registry's sparse index;
//! - `git` checks out git dependencies;
//! - `solver` picks versions and features (PubGrub, with Cargo's semantics);
//! - `lockfile` reads and writes `Cargo.lock`;
//! - `island` glues the above to Yarn's install pipeline;
//! - `toolchain` provisions `rustc` and `cargo` themselves;
//! - `editor` lets the commands editing dependencies work on `Cargo.toml`.
//!
//! The `cargo` linker (`linker::cargo`) then lays the crates out as a
//! vendor directory that Cargo builds from, offline.

use std::{collections::BTreeMap, sync::Arc};

use zpm_config::Setting;
use zpm_primitives::{CargoGitSource, CargoReq, CargoRequirement, CargoSemver};
use zpm_utils::Path;

use crate::{http::HttpClient, project::Project};

pub mod editor;
pub mod git;
pub mod index;
pub mod island;
pub mod lockfile;
pub mod manifest;
pub mod solver;
pub mod toolchain;

/// The source id Cargo uses for crates.io in its lockfiles, whatever the
/// protocol (git or sparse) the index is accessed through.
pub const CRATES_IO_SOURCE: &str
    = "registry+https://github.com/rust-lang/crates.io-index";

/// A registry, by the URL of its sparse index (with a trailing slash).
/// `None` is the default registry: crates.io, or what `cargoRegistryServer`
/// replaces it with.
pub type RegistryIndex = Option<String>;

/// `major.minor.patch` of a Rust release; what `rust-version` fields hold.
pub type RustVersion = (u64, u64, u64);

pub fn parse_rust_version(src: &str) -> Option<RustVersion> {
    let src
        = src.trim().split(['-', '+']).next()?;

    let mut components
        = src.split('.').map(|component| component.parse::<u64>().ok());

    let major = components.next()??;
    let minor = components.next().unwrap_or(Some(0))?;
    let patch = components.next().unwrap_or(Some(0))?;

    components.next().is_none().then_some((major, minor, patch))
}

/// The URLs crates.io is known by: its git index (which is what
/// lockfiles and published manifests name) and its sparse one.
const CRATES_IO_INDEXES: [&str; 2] = [
    "https://github.com/rust-lang/crates.io-index/",
    "https://index.crates.io/",
];

/// The registries crates may come from.
#[derive(Clone, Debug, Default)]
pub struct Registries {
    /// Index of the default registry
    pub default_index: String,

    /// The registries manifests refer to by name (`registry = "<name>"`)
    pub names: BTreeMap<String, String>,

    /// The `Authorization` header sent to each index
    pub tokens: BTreeMap<String, String>,
}

impl Registries {
    pub fn new(project: &Project) -> Registries {
        let settings
            = &project.config.settings;

        let mut registries = Registries {
            default_index: index::normalize_index_url(&settings.cargo_registry_server.value),
            ..Default::default()
        };

        if let Some(token) = &settings.cargo_auth_token.value {
            registries.tokens.insert(registries.default_index.clone(), token.value.clone());
        }

        // Rust projects already name their registries in Cargo's own
        // configuration, which Cargo needs to read the manifests anyway
        let cargo_config = project.project_cwd.with_join_str(".cargo/config.toml").fs_read_text().ok()
            .and_then(|text| text.parse::<toml_edit::DocumentMut>().ok());

        let cargo_registries = cargo_config.as_ref()
            .and_then(|document| document.get("registries"))
            .and_then(|item| item.as_table_like());

        for (name, registry) in cargo_registries.into_iter().flat_map(|table| table.iter()) {
            if let Some(index) = registry.as_table_like().and_then(|table| table.get("index")).and_then(|item| item.as_str()) {
                registries.names.insert(name.to_string(), index::normalize_index_url(index));
            }
        }

        for (name, registry) in &settings.cargo_registries {
            let Some(index) = registry.cargo_registry_server.value.as_deref().map(index::normalize_index_url).or_else(|| registries.names.get(name).cloned()) else {
                continue;
            };

            if let Some(token) = &registry.cargo_auth_token.value {
                registries.tokens.insert(index.clone(), token.value.clone());
            }

            registries.names.insert(name.clone(), index);
        }

        registries
    }

    /// The registry an index URL designates.
    pub fn from_url(&self, url: &str) -> RegistryIndex {
        let url
            = index::normalize_index_url(url);

        match url == self.default_index || CRATES_IO_INDEXES.contains(&url.as_str()) {
            true => None,
            false => Some(url),
        }
    }

    pub fn by_name(&self, name: &str) -> Option<RegistryIndex> {
        match name {
            "crates-io" => Some(None),
            _ => self.names.get(name).map(|index| self.from_url(index)),
        }
    }

    pub fn index_of<'a>(&'a self, registry: &'a RegistryIndex) -> &'a str {
        registry.as_deref().unwrap_or(&self.default_index)
    }

    pub fn token_of(&self, registry: &RegistryIndex) -> Option<&str> {
        self.tokens.get(self.index_of(registry)).map(|token| token.as_str())
    }
}

/// The source id Cargo gives a registry in its lockfiles.
pub fn registry_source_id(registry: &RegistryIndex) -> String {
    match registry {
        Some(index) => format!("sparse+{}", index),
        None => CRATES_IO_SOURCE.to_string(),
    }
}

/// What the Cargo support needs from the project. Unlike the install
/// context it owns its data, so it can be moved to the solver's thread.
#[derive(Clone, Debug)]
pub struct CargoContext {
    pub http_client: Arc<HttpClient>,
    pub registries: Registries,

    /// Where the index files and the git checkouts are kept
    pub cache_dir: Path,

    pub project_cwd: Path,
    pub approved_git_repositories: Vec<Setting<String>>,
}

impl CargoContext {
    pub fn new(project: &Project) -> CargoContext {
        CargoContext {
            http_client: project.http_client.clone(),
            registries: Registries::new(project),
            cache_dir: project.config.settings.global_folder.value.with_join_str("cargo"),
            project_cwd: project.project_cwd.clone(),
            approved_git_repositories: project.config.settings.approved_git_repositories.clone(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DependencyKind {
    Normal,
    Build,
    Development,
}

impl DependencyKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            DependencyKind::Normal => "normal",
            DependencyKind::Build => "build",
            DependencyKind::Development => "dev",
        }
    }
}

/// Where a dependency is to be looked for.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DependencySource {
    Registry(RegistryIndex),
    Git(CargoGitSource),

    /// Absolute path of the folder holding the crate's manifest
    Path(Path),
}

#[derive(Clone, Debug)]
pub struct Dependency {
    /// The name the dependent refers to the crate by
    pub name_in_toml: String,

    /// The name of the crate; differs from `name_in_toml` for renamed
    /// dependencies (`package = "..."`)
    pub package: String,

    pub requirement: CargoRequirement,
    pub req: CargoReq,

    pub features: Vec<String>,
    pub default_features: bool,
    pub optional: bool,

    pub kind: DependencyKind,

    /// The `cfg(...)` expression or target triple the dependency is
    /// restricted to. Lockfiles hold the dependencies of every platform, so
    /// this is only informative as far as resolving is concerned.
    pub target: Option<String>,

    pub source: DependencySource,
}

/// Where a crate comes from.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CrateSource {
    Registry(RegistryIndex),

    Git {
        source: CargoGitSource,
        commit: String,
    },

    /// Absolute path of the folder holding the crate's manifest
    Path(Path),
}

/// What the resolver needs to know about one version of a crate.
#[derive(Clone, Debug)]
pub struct Summary {
    pub name: String,
    pub version: CargoSemver,
    pub source: CrateSource,
    pub dependencies: Vec<Dependency>,

    /// The features the crate declares, including the implicit ones Cargo
    /// creates for the optional dependencies no feature refers to through
    /// the `dep:` syntax
    pub features: BTreeMap<String, Vec<String>>,

    /// Hex sha256 of the `.crate` file; registry crates only
    pub checksum: Option<String>,

    pub yanked: bool,
    pub links: Option<String>,

    /// The oldest Rust release the crate claims to build with
    pub rust_version: Option<RustVersion>,

    /// Set on the registry crates a `[replace]` table swaps for another
    /// one. The dependencies and features are then those of the
    /// replacement, which is what gets built.
    pub replaced_by: Option<Arc<Summary>>,
}

/// What a `[patch]` table applies to.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PatchTarget {
    Registry(RegistryIndex),

    /// A git repository, by its canonical URL
    Git(String),
}

/// Cargo doesn't tell apart the various ways a repository URL can be
/// written.
pub fn canonical_git_url(url: &str) -> String {
    let url
        = url.trim_end_matches('/');

    url.strip_suffix(".git").unwrap_or(url).to_ascii_lowercase()
}

/// Adds the implicit features of the optional dependencies.
pub fn add_implicit_features(features: &mut BTreeMap<String, Vec<String>>, dependencies: &[Dependency]) {
    let explicit = features.values()
        .flatten()
        .filter_map(|value| value.strip_prefix("dep:"))
        .map(|name| name.to_string())
        .collect::<std::collections::BTreeSet<_>>();

    for dependency in dependencies {
        if !dependency.optional || explicit.contains(&dependency.name_in_toml) {
            continue;
        }

        features.entry(dependency.name_in_toml.clone())
            .or_insert_with(|| vec![format!("dep:{}", dependency.name_in_toml)]);
    }
}

/// One of the things a feature can enable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FeatureValue<'a> {
    /// `feature`
    Feature(&'a str),

    /// `dep:name`
    Dependency(&'a str),

    /// `name/feature`, or `name?/feature` when weak
    DependencyFeature {
        dependency: &'a str,
        feature: &'a str,
        weak: bool,
    },
}

impl<'a> FeatureValue<'a> {
    pub fn parse(value: &'a str) -> FeatureValue<'a> {
        if let Some(dependency) = value.strip_prefix("dep:") {
            return FeatureValue::Dependency(dependency);
        }

        match value.split_once('/') {
            Some((dependency, feature)) => match dependency.strip_suffix('?') {
                Some(dependency) => FeatureValue::DependencyFeature {dependency, feature, weak: true},
                None => FeatureValue::DependencyFeature {dependency, feature, weak: false},
            },

            None => FeatureValue::Feature(value),
        }
    }
}
