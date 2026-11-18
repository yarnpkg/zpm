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
//! - `island` glues the above to Yarn's install pipeline.
//!
//! The `cargo` linker (`linker::cargo`) then lays the crates out as a
//! vendor directory that Cargo builds from, offline.

use std::{collections::BTreeMap, sync::Arc};

use zpm_config::Setting;
use zpm_primitives::{CargoGitSource, CargoReq, CargoRequirement, CargoSemver};
use zpm_utils::Path;

use crate::{http::HttpClient, project::Project};

pub mod git;
pub mod index;
pub mod island;
pub mod lockfile;
pub mod manifest;
pub mod solver;

/// The source id Cargo uses for crates.io in its lockfiles, whatever the
/// protocol (git or sparse) the index is accessed through.
pub const CRATES_IO_SOURCE: &str
    = "registry+https://github.com/rust-lang/crates.io-index";

/// What the Cargo support needs from the project. Unlike the install
/// context it owns its data, so it can be moved to the solver's thread.
#[derive(Clone, Debug)]
pub struct CargoContext {
    pub http_client: Arc<HttpClient>,

    /// Root of the sparse index, with a trailing slash
    pub index_url: String,

    /// Where the index files and the git checkouts are kept
    pub cache_dir: Path,

    pub project_cwd: Path,
    pub approved_git_repositories: Vec<Setting<String>>,
}

impl CargoContext {
    pub fn new(project: &Project) -> CargoContext {
        CargoContext {
            http_client: project.http_client.clone(),
            index_url: index::normalize_index_url(&project.config.settings.cargo_registry_server.value),
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
    Registry,
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
    Registry,

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
