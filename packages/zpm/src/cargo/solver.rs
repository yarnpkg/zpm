//! Version and feature resolution.
//!
//! Cargo's rules are quite different from the ones PyPI islands follow:
//!
//! - A graph may hold several versions of a crate, as long as they aren't
//!   semver-compatible with each other: `1.2.0` and `1.9.0` can't coexist,
//!   `1.9.0` and `2.0.0` can. Each (crate, compatibility bucket) pair is
//!   thus its own package as far as the solver is concerned.
//!
//! - Features decide which optional dependencies are part of the graph,
//!   and they're unified: a crate is built once, with the union of the
//!   features its dependents ask for. Like PyPI extras, each requested
//!   feature is a proxy package pinned to the version of its crate, whose
//!   dependencies are what the feature enables.
//!
//! - A requirement spanning several buckets (`>=1, <3`) is yet another
//!   proxy, with one version per bucket it can be satisfied from.
//!
//! - `[patch]` tables add candidates to a source (the fork of a crate,
//!   say), which are preferred over the published versions whenever they
//!   satisfy the requirement; `[replace]` swaps one precise version for
//!   another crate.
//!
//! - When the workspace opts in (resolver 3), the versions supporting its
//!   Rust version are preferred over newer ones that don't.
//!
//! The result is what `cargo generate-lockfile` computes: a resolution
//! valid for every platform at once, with the workspace members' features
//! all enabled and their dev-dependencies included. Like Cargo, this
//! over-approximates on purpose - weak dependency features (`dep?/feat`)
//! do pull the dependency in, for instance - so that the same lockfile can
//! be used whatever gets built afterwards.

use std::{cell::RefCell, cmp::Reverse, collections::{BTreeMap, BTreeSet, HashMap}, fmt, sync::Arc};

use pubgrub::{Dependencies, DependencyProvider, PackageResolutionStatistics, Ranges, Reporter};
use zpm_primitives::{CargoBucket, CargoGitSource, CargoReq, CargoSemver};
use zpm_utils::{Path, ToFileString};

use super::{CrateSource, Dependency, DependencyKind, DependencySource, FeatureValue, PatchTarget, RegistryIndex, RustVersion, Summary, canonical_git_url, git::GitPackage, index::CrateVersions, manifest::Replacement};

pub type VersionSet = Ranges<CargoSemver>;

#[derive(Clone, Debug)]
pub struct SolverError(pub String);

impl fmt::Display for SolverError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for SolverError {}

/// A git repository at the commit the resolution uses.
#[derive(Clone, Debug)]
pub struct GitRepository {
    pub commit: String,
    pub packages: BTreeMap<String, GitPackage>,
}

/// Where the solver gets its data from. The install uses the registry
/// index, the disk and git; tests use in-memory fixtures.
pub trait CrateCatalog {
    fn registry(&self, registry: &RegistryIndex, name: &str) -> Result<Arc<CrateVersions>, SolverError>;
    fn path(&self, dir: &Path) -> Result<Arc<Summary>, SolverError>;
    fn git(&self, source: &CargoGitSource) -> Result<Arc<GitRepository>, SolverError>;

    /// Hints that those crates are about to be requested
    fn prefetch(&self, _crates: &[(RegistryIndex, String)]) {}
}

/// Identity of a crate in the graph.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PackageKey {
    Registry {
        registry: RegistryIndex,
        name: String,
        bucket: CargoBucket,
    },

    Git {
        name: String,
        source: CargoGitSource,
    },

    Path {
        name: String,
        dir: Path,
    },
}

impl PackageKey {
    pub fn name(&self) -> &str {
        match self {
            PackageKey::Registry {name, ..} => name,
            PackageKey::Git {name, ..} => name,
            PackageKey::Path {name, ..} => name,
        }
    }
}

impl fmt::Display for PackageKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PackageKey::Registry {registry: None, name, bucket} => write!(f, "{} ({})", name, bucket),
            PackageKey::Registry {registry: Some(registry), name, bucket} => write!(f, "{} ({}, from {})", name, bucket, registry),
            PackageKey::Git {name, source} => write!(f, "{} ({})", name, source.as_str()),
            PackageKey::Path {name, dir} => write!(f, "{} ({})", name, dir.to_file_string()),
        }
    }
}

/// A registry requirement that can be satisfied from several buckets.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct WideRequirement {
    registry: RegistryIndex,
    name: String,
    requirement: String,
    features: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum SolverPackage {
    Root,
    Crate(PackageKey),
    Feature(PackageKey, String),
    Wide(WideRequirement),
}

impl fmt::Display for SolverPackage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SolverPackage::Root => write!(f, "the workspace"),
            SolverPackage::Crate(key) => write!(f, "{}", key),
            SolverPackage::Feature(key, feature) => write!(f, "{} with the `{}` feature", key, feature),
            SolverPackage::Wide(wide) => write!(f, "{} ({})", wide.name, wide.requirement),
        }
    }
}

/// The versions resolutions should stick to when they can.
#[derive(Clone, Debug, Default)]
pub struct LockedVersions {
    pub registry: BTreeMap<String, BTreeSet<CargoSemver>>,
}

/// What a resolution is computed from, besides the crates themselves.
#[derive(Clone, Debug, Default)]
pub struct SolveOptions {
    /// Folders of the workspace members
    pub members: BTreeSet<Path>,

    pub locked: LockedVersions,

    /// The Rust versions the workspace has to build with. Candidates are
    /// tried from those supporting the most of them to those supporting
    /// the fewest; empty when the workspace doesn't ask for it.
    pub rust_versions: Vec<RustVersion>,

    pub patches: Vec<(PatchTarget, Dependency)>,
    pub replacements: Vec<Replacement>,
}

/// The crate a patch or a replacement points to.
fn source_summary<C: CrateCatalog>(catalog: &C, dependency: &Dependency) -> Result<Arc<Summary>, SolverError> {
    match &dependency.source {
        DependencySource::Path(dir) => {
            catalog.path(dir)
        },

        DependencySource::Git(source) => {
            catalog.git(source)?.packages.get(&dependency.package)
                .map(|package| package.summary.clone())
                .ok_or_else(|| SolverError(format!("No crate named {} was found in {}", dependency.package, source.url())))
        },

        DependencySource::Registry(_) => {
            Err(SolverError(format!("The patch for {} must point to a path or to a git repository", dependency.package)))
        },
    }
}

/// The package a crate living on the disk or in a repository stands for.
fn key_of(summary: &Summary) -> PackageKey {
    match &summary.source {
        CrateSource::Registry(registry) => PackageKey::Registry {
            registry: registry.clone(),
            name: summary.name.clone(),
            bucket: summary.version.bucket(),
        },

        CrateSource::Git {source, ..} => PackageKey::Git {
            name: summary.name.clone(),
            source: source.clone(),
        },

        CrateSource::Path(dir) => PackageKey::Path {
            name: summary.name.clone(),
            dir: dir.clone(),
        },
    }
}

/// The registry crate a `[replace]` entry applies to keeps its identity,
/// but gets built from the replacement.
fn replaced(original: &Summary, replacement: &Arc<Summary>) -> Summary {
    Summary {
        dependencies: replacement.dependencies.clone(),
        features: replacement.features.clone(),
        links: replacement.links.clone(),
        rust_version: replacement.rust_version,
        replaced_by: Some(replacement.clone()),
        ..original.clone()
    }
}

fn root_version() -> CargoSemver {
    CargoSemver::parse("0.0.0").unwrap()
}

/// The versions among `candidates` (sorted, lowest first) that `predicate`
/// accepts. Consecutive matches are merged in a single interval, which is
/// fine as nothing exists between two consecutive candidates.
fn version_set<'a>(candidates: impl Iterator<Item = &'a CargoSemver>, predicate: impl Fn(&CargoSemver) -> bool) -> VersionSet {
    let mut set
        = Ranges::empty();

    let mut run: Option<(&CargoSemver, &CargoSemver)>
        = None;

    let flush = |run: Option<(&CargoSemver, &CargoSemver)>, set: &mut VersionSet| {
        if let Some((low, high)) = run {
            *set = set.union(&Ranges::from_range_bounds(low.clone()..=high.clone()));
        }
    };

    for candidate in candidates {
        match (predicate(candidate), run) {
            (true, Some((low, _))) => run = Some((low, candidate)),
            (true, None) => run = Some((candidate, candidate)),
            (false, _) => flush(run.take(), &mut set),
        }
    }

    flush(run, &mut set);

    set
}

fn constrain(constraints: &mut HashMap<SolverPackage, VersionSet>, package: SolverPackage, set: VersionSet) {
    match constraints.get_mut(&package) {
        Some(existing) => *existing = existing.intersection(&set),
        None => {constraints.insert(package, set);},
    }
}

/// The features a dependent asks of a dependency: the ones listed in the
/// manifest, the ones enabled on it by the dependent's own features, and
/// `default` unless opted out.
fn requested_features(dependency: &Dependency, extra: &BTreeSet<String>) -> Vec<String> {
    let mut features = dependency.features.iter()
        .chain(extra)
        .cloned()
        .collect::<BTreeSet<_>>();

    if dependency.default_features {
        features.insert("default".to_string());
    }

    features.into_iter().collect()
}

/// Whether a crate can be asked for a feature. Cargo rejects the versions
/// lacking a feature a dependent asks for, which is what lets the solver
/// fall back on the versions that have it.
fn has_feature(summary: &Summary, value: &str) -> bool {
    match FeatureValue::parse(value) {
        FeatureValue::Feature(name) => name == "default" || summary.features.contains_key(name),
        FeatureValue::Dependency(name) => summary.dependencies.iter().any(|dependency| dependency.name_in_toml == name),
        FeatureValue::DependencyFeature {dependency: name, ..} => summary.dependencies.iter().any(|dependency| dependency.name_in_toml == name),
    }
}

/// The dependencies a crate has once the given feature values are enabled,
/// along with the features each of them is asked for by those values.
fn activated_dependencies<'a>(summary: &'a Summary, enabled: &BTreeSet<String>, is_member: bool) -> Vec<(&'a Dependency, BTreeSet<String>)> {
    let mut by_name: BTreeMap<&str, BTreeSet<String>>
        = BTreeMap::new();

    for value in enabled {
        match FeatureValue::parse(value) {
            FeatureValue::Feature(_) => {},

            FeatureValue::Dependency(name) => {
                by_name.entry(name).or_default();
            },

            FeatureValue::DependencyFeature {dependency, feature, ..} => {
                by_name.entry(dependency).or_default().insert(feature.to_string());
            },
        }
    }

    summary.dependencies.iter()
        .filter(|dependency| is_member || dependency.kind != DependencyKind::Development)
        .filter(|dependency| !dependency.optional || by_name.contains_key(dependency.name_in_toml.as_str()))
        .map(|dependency| (dependency, by_name.get(dependency.name_in_toml.as_str()).cloned().unwrap_or_default()))
        .collect()
}

enum EdgeTarget {
    Single(PackageKey, VersionSet),
    Wide(WideRequirement),
}

struct RegistryCrate {
    /// Lowest first; yanked versions are only there when locked
    candidates: Vec<Arc<Summary>>,
}

struct CargoProvider<'a, C: CrateCatalog> {
    catalog: &'a C,
    options: &'a SolveOptions,
    patches: HashMap<(PatchTarget, String), Vec<Arc<Summary>>>,
    replacements: HashMap<(String, CargoSemver), Arc<Summary>>,
    registry: RefCell<HashMap<(RegistryIndex, String), Arc<RegistryCrate>>>,
    discovery: RefCell<HashMap<String, usize>>,
}

impl<'a, C: CrateCatalog> CargoProvider<'a, C> {
    fn registry_crate(&self, registry: &RegistryIndex, name: &str) -> Result<Arc<RegistryCrate>, SolverError> {
        let cache_key
            = (registry.clone(), name.to_string());

        if let Some(registry_crate) = self.registry.borrow().get(&cache_key) {
            return Ok(registry_crate.clone());
        }

        let versions
            = self.catalog.registry(registry, name)?;

        let locked
            = self.options.locked.registry.get(name);

        let patches = self.patches.get(&(PatchTarget::Registry(registry.clone()), name.to_string()))
            .map(|patches| patches.as_slice())
            .unwrap_or_default();

        // Patches are candidates like the others, except that they hide
        // the published version they share their version with
        let mut candidates = versions.versions.iter()
            .filter(|summary| !summary.yanked || locked.is_some_and(|locked| locked.contains(&summary.version)))
            .filter(|summary| !patches.iter().any(|patch| patch.version == summary.version))
            .map(|summary| match self.replacements.get(&(name.to_string(), summary.version.clone())) {
                Some(replacement) => Arc::new(replaced(summary, replacement)),
                None => summary.clone(),
            })
            .chain(patches.iter().cloned())
            .collect::<Vec<_>>();

        candidates.sort_by(|a, b| a.version.cmp(&b.version));

        let registry_crate
            = Arc::new(RegistryCrate {candidates});

        self.registry.borrow_mut().insert(cache_key, registry_crate.clone());

        Ok(registry_crate)
    }

    fn prefetch<'d>(&self, dependencies: impl Iterator<Item = &'d Dependency>) {
        let registry
            = self.registry.borrow();

        let crates = dependencies
            .filter_map(|dependency| match &dependency.source {
                DependencySource::Registry(registry) => Some((registry.clone(), dependency.package.clone())),
                _ => None,
            })
            .filter(|key| !registry.contains_key(key))
            .collect::<BTreeSet<_>>();

        drop(registry);

        if crates.len() > 1 {
            self.catalog.prefetch(&crates.into_iter().collect::<Vec<_>>());
        }
    }

    fn is_member(&self, key: &PackageKey) -> bool {
        matches!(key, PackageKey::Path {dir, ..} if self.options.members.contains(dir))
    }

    /// How many of the workspace's Rust versions a crate supports.
    fn rust_compatibility(&self, summary: &Summary) -> usize {
        self.options.rust_versions.iter()
            .filter(|rust_version| summary.rust_version.is_none_or(|required| required <= **rust_version))
            .count()
    }

    /// How much a candidate is wanted, the same way Cargo ranks them:
    /// what the lockfile holds and the patches come first, then what
    /// supports the workspace's Rust version, then the highest version.
    fn preference(&self, summary: &Summary) -> (bool, usize, CargoSemver) {
        let is_locked
            = self.options.locked.registry.get(&summary.name).is_some_and(|locked| locked.contains(&summary.version));

        let is_patch
            = !matches!(summary.source, CrateSource::Registry(_));

        (is_locked || is_patch, self.rust_compatibility(summary), summary.version.clone())
    }

    /// The versions of a package, preferred ones first.
    fn candidates(&self, key: &PackageKey) -> Result<Vec<Arc<Summary>>, SolverError> {
        match key {
            PackageKey::Registry {registry, name, bucket} => {
                let registry_crate
                    = self.registry_crate(registry, name)?;

                let mut candidates = registry_crate.candidates.iter()
                    .filter(|summary| &summary.version.bucket() == bucket)
                    .cloned()
                    .collect::<Vec<_>>();

                candidates.sort_by_key(|summary| Reverse(self.preference(summary)));

                Ok(candidates)
            },

            PackageKey::Git {name, source} => {
                let repository
                    = self.catalog.git(source)?;

                let package = repository.packages.get(name)
                    .ok_or_else(|| SolverError(format!("No crate named {} was found in {}", name, source.url())))?;

                Ok(vec![package.summary.clone()])
            },

            PackageKey::Path {dir, ..} => {
                Ok(vec![self.catalog.path(dir)?])
            },
        }
    }

    fn summary(&self, key: &PackageKey, version: &CargoSemver) -> Result<Arc<Summary>, SolverError> {
        self.candidates(key)?.into_iter()
            .find(|summary| &summary.version == version)
            .ok_or_else(|| SolverError(format!("Version {} of {} went missing during the resolution", version, key)))
    }

    /// What a dependency resolves to: a package and the versions the
    /// requirement accepts.
    fn edge_target(&self, dependency: &Dependency, features: &[String]) -> Result<EdgeTarget, SolverError> {
        match &dependency.source {
            DependencySource::Registry(registry) => {
                let registry_crate
                    = self.registry_crate(registry, &dependency.package)?;

                let buckets = registry_crate.candidates.iter()
                    .filter(|summary| dependency.req.matches(&summary.version))
                    .map(|summary| summary.version.bucket())
                    .collect::<BTreeSet<_>>();

                if buckets.len() > 1 {
                    return Ok(EdgeTarget::Wide(WideRequirement {
                        registry: registry.clone(),
                        name: dependency.package.clone(),
                        requirement: dependency.requirement.as_str().to_string(),
                        features: features.to_vec(),
                    }));
                }

                // Without any match the dependency can't be satisfied; the
                // empty set makes the solver try another dependent
                let bucket
                    = buckets.into_iter().next().unwrap_or(CargoBucket::Patch(0));

                let set
                    = bucket_set(&registry_crate, &bucket, &dependency.req);

                Ok(EdgeTarget::Single(PackageKey::Registry {registry: registry.clone(), name: dependency.package.clone(), bucket}, set))
            },

            DependencySource::Git(source) => {
                // A patched repository gives way to its patch
                let patch = self.patches.get(&(PatchTarget::Git(canonical_git_url(source.url())), dependency.package.clone()))
                    .and_then(|patches| patches.first());

                if let Some(patch) = patch {
                    return Ok(EdgeTarget::Single(key_of(patch), Ranges::singleton(patch.version.clone())));
                }

                let key = PackageKey::Git {
                    name: dependency.package.clone(),
                    source: source.clone(),
                };

                let version
                    = self.candidates(&key)?[0].version.clone();

                Ok(EdgeTarget::Single(key, Ranges::singleton(version)))
            },

            DependencySource::Path(dir) => {
                let summary
                    = self.catalog.path(dir)?;

                let key = PackageKey::Path {
                    name: summary.name.clone(),
                    dir: dir.clone(),
                };

                Ok(EdgeTarget::Single(key, Ranges::singleton(summary.version.clone())))
            },
        }
    }

    fn add_edge(&self, constraints: &mut HashMap<SolverPackage, VersionSet>, dependency: &Dependency, extra: &BTreeSet<String>) -> Result<(), SolverError> {
        let features
            = requested_features(dependency, extra);

        match self.edge_target(dependency, &features)? {
            EdgeTarget::Single(key, set) => {
                for feature in features {
                    constrain(constraints, SolverPackage::Feature(key.clone(), feature), set.clone());
                }

                constrain(constraints, SolverPackage::Crate(key), set);
            },

            EdgeTarget::Wide(wide) => {
                constrain(constraints, SolverPackage::Wide(wide), Ranges::full());
            },
        }

        Ok(())
    }

    fn root_dependencies(&self) -> Result<HashMap<SolverPackage, VersionSet>, SolverError> {
        let mut constraints
            = HashMap::new();

        for dir in &self.options.members {
            let summary
                = self.catalog.path(dir)?;

            let key = PackageKey::Path {
                name: summary.name.clone(),
                dir: dir.clone(),
            };

            let set
                = Ranges::singleton(summary.version.clone());

            // Lockfiles cover whatever may get built later on, hence
            // every feature of the workspace members
            for feature in summary.features.keys() {
                constrain(&mut constraints, SolverPackage::Feature(key.clone(), feature.clone()), set.clone());
            }

            constrain(&mut constraints, SolverPackage::Crate(key), set);
        }

        Ok(constraints)
    }

    fn crate_dependencies(&self, key: &PackageKey, version: &CargoSemver) -> Result<HashMap<SolverPackage, VersionSet>, SolverError> {
        let summary
            = self.summary(key, version)?;

        let is_member
            = self.is_member(key);

        let dependencies = summary.dependencies.iter()
            .filter(|dependency| !dependency.optional)
            .filter(|dependency| is_member || dependency.kind != DependencyKind::Development)
            .collect::<Vec<_>>();

        self.prefetch(dependencies.iter().copied());

        let mut constraints
            = HashMap::new();

        for dependency in dependencies {
            self.add_edge(&mut constraints, dependency, &BTreeSet::new())?;
        }

        Ok(constraints)
    }

    fn feature_dependencies(&self, key: &PackageKey, value: &str, version: &CargoSemver) -> Result<HashMap<SolverPackage, VersionSet>, SolverError> {
        let summary
            = self.summary(key, version)?;

        let is_member
            = self.is_member(key);

        let pinned
            = Ranges::singleton(version.clone());

        let mut constraints
            = HashMap::new();

        constrain(&mut constraints, SolverPackage::Crate(key.clone()), pinned.clone());

        let named = |name: &str| {
            summary.dependencies.iter()
                .filter(|dependency| dependency.name_in_toml == name)
                .filter(|dependency| is_member || dependency.kind != DependencyKind::Development)
                .collect::<Vec<_>>()
        };

        match FeatureValue::parse(value) {
            FeatureValue::Feature(name) => {
                for value in summary.features.get(name).into_iter().flatten() {
                    constrain(&mut constraints, SolverPackage::Feature(key.clone(), value.clone()), pinned.clone());
                }
            },

            FeatureValue::Dependency(name) => {
                for dependency in named(name) {
                    self.add_edge(&mut constraints, dependency, &BTreeSet::new())?;
                }
            },

            FeatureValue::DependencyFeature {dependency: name, feature, weak} => {
                let extra
                    = BTreeSet::from([feature.to_string()]);

                let dependencies
                    = named(name);

                // `name/feature` also enables the feature named after an
                // optional dependency, when there is one
                let enables_implicit_feature
                    = !weak && dependencies.iter().any(|dependency| dependency.optional) && summary.features.contains_key(name);

                if enables_implicit_feature {
                    constrain(&mut constraints, SolverPackage::Feature(key.clone(), name.to_string()), pinned.clone());
                }

                for dependency in dependencies {
                    self.add_edge(&mut constraints, dependency, &extra)?;
                }
            },
        }

        Ok(constraints)
    }

    /// The buckets a wide requirement can be satisfied from, preferred
    /// ones first.
    fn wide_buckets(&self, wide: &WideRequirement) -> Result<Vec<CargoBucket>, SolverError> {
        let registry_crate
            = self.registry_crate(&wide.registry, &wide.name)?;

        let req
            = CargoReq::parse(&wide.requirement)
                .ok_or_else(|| SolverError(format!("Invalid requirement: {}", wide.requirement)))?;

        // A bucket is as good as the best version it can offer
        let mut candidates = registry_crate.candidates.iter()
            .filter(|summary| req.matches(&summary.version))
            .collect::<Vec<_>>();

        candidates.sort_by_key(|summary| Reverse(self.preference(summary)));

        let mut seen
            = BTreeSet::new();

        Ok(candidates.into_iter()
            .map(|summary| summary.version.bucket())
            .filter(|bucket| seen.insert(bucket.clone()))
            .collect())
    }

    fn wide_dependencies(&self, wide: &WideRequirement, version: &CargoSemver) -> Result<HashMap<SolverPackage, VersionSet>, SolverError> {
        let registry_crate
            = self.registry_crate(&wide.registry, &wide.name)?;

        let req
            = CargoReq::parse(&wide.requirement)
                .ok_or_else(|| SolverError(format!("Invalid requirement: {}", wide.requirement)))?;

        let bucket
            = version.bucket();

        let set
            = bucket_set(&registry_crate, &bucket, &req);

        let key = PackageKey::Registry {
            registry: wide.registry.clone(),
            name: wide.name.clone(),
            bucket,
        };

        let mut constraints
            = HashMap::new();

        for feature in &wide.features {
            constrain(&mut constraints, SolverPackage::Feature(key.clone(), feature.clone()), set.clone());
        }

        constrain(&mut constraints, SolverPackage::Crate(key), set);

        Ok(constraints)
    }
}

fn bucket_set(registry_crate: &RegistryCrate, bucket: &CargoBucket, req: &CargoReq) -> VersionSet {
    let versions = registry_crate.candidates.iter()
        .map(|summary| &summary.version)
        .filter(|version| &version.bucket() == bucket);

    version_set(versions, |version| req.matches(version))
}

impl<'a, C: CrateCatalog> DependencyProvider for CargoProvider<'a, C> {
    type P = SolverPackage;
    type V = CargoSemver;
    type VS = VersionSet;
    type Priority = (u8, Reverse<usize>);
    type M = String;
    type Err = SolverError;

    fn prioritize(&self, package: &SolverPackage, range: &VersionSet, stats: &PackageResolutionStatistics) -> Self::Priority {
        let name = match package {
            SolverPackage::Root => return (u8::MAX, Reverse(0)),
            SolverPackage::Crate(key) | SolverPackage::Feature(key, _) => key.name(),
            SolverPackage::Wide(wide) => wide.name.as_str(),
        };

        let discovery = {
            let mut discovery
                = self.discovery.borrow_mut();

            let next
                = discovery.len();

            *discovery.entry(name.to_string()).or_insert(next)
        };

        // Same tiers as the other islands: what has a single possible
        // answer goes first, then what was locked, then what caused
        // conflicts, then the rest in the order it was met
        let is_pinned
            = range.as_singleton().is_some() || matches!(package, SolverPackage::Crate(PackageKey::Git {..} | PackageKey::Path {..}));

        let tier = if is_pinned {
            3
        } else if self.options.locked.registry.contains_key(name) {
            2
        } else if stats.conflict_count() > 0 {
            1
        } else {
            0
        };

        (tier, Reverse(discovery))
    }

    fn choose_version(&self, package: &SolverPackage, range: &VersionSet) -> Result<Option<CargoSemver>, SolverError> {
        match package {
            SolverPackage::Root => {
                Ok(Some(root_version()).filter(|version| range.contains(version)))
            },

            SolverPackage::Crate(key) => {
                Ok(self.candidates(key)?.into_iter()
                    .map(|summary| summary.version.clone())
                    .find(|version| range.contains(version)))
            },

            SolverPackage::Feature(key, value) => {
                Ok(self.candidates(key)?.into_iter()
                    .filter(|summary| has_feature(summary, value))
                    .map(|summary| summary.version.clone())
                    .find(|version| range.contains(version)))
            },

            SolverPackage::Wide(wide) => {
                Ok(self.wide_buckets(wide)?.iter()
                    .map(CargoSemver::bucket_floor)
                    .find(|version| range.contains(version)))
            },
        }
    }

    fn get_dependencies(&self, package: &SolverPackage, version: &CargoSemver) -> Result<Dependencies<SolverPackage, VersionSet, String>, SolverError> {
        let constraints = match package {
            SolverPackage::Root => self.root_dependencies()?,
            SolverPackage::Crate(key) => self.crate_dependencies(key, version)?,
            SolverPackage::Feature(key, value) => self.feature_dependencies(key, value, version)?,
            SolverPackage::Wide(wide) => self.wide_dependencies(wide, version)?,
        };

        Ok(Dependencies::Available(constraints.into_iter().collect()))
    }
}

#[derive(Clone, Debug)]
pub struct SolvedEdge {
    pub dependency: Dependency,

    /// Index of the dependency in `Solution::packages`
    pub target: usize,
}

#[derive(Clone, Debug)]
pub struct SolvedPackage {
    pub key: PackageKey,
    pub summary: Arc<Summary>,
    pub is_member: bool,

    /// The feature values enabled on the package (`std`, `dep:foo`,
    /// `foo/bar`), `default` included when a dependent asked for it
    pub features: BTreeSet<String>,

    pub edges: Vec<SolvedEdge>,
}

#[derive(Clone, Debug, Default)]
pub struct Solution {
    pub packages: Vec<SolvedPackage>,

    /// The patches that didn't make it into the graph, because nothing
    /// depends on the crate or because another version was preferred.
    /// Cargo keeps track of them in its lockfile.
    pub unused_patches: Vec<Arc<Summary>>,
}

fn describe_failure<C: CrateCatalog>(error: pubgrub::PubGrubError<CargoProvider<'_, C>>) -> String {
    match error {
        pubgrub::PubGrubError::NoSolution(mut derivation_tree) => {
            derivation_tree.collapse_no_versions();

            let report
                = pubgrub::DefaultStringReporter::report(&derivation_tree);

            format!("No solution found.\n\n{}", report.lines().take(40).collect::<Vec<_>>().join("\n"))
        },

        pubgrub::PubGrubError::ErrorChoosingVersion {source, ..} |
        pubgrub::PubGrubError::ErrorRetrievingDependencies {source, ..} => {
            source.to_string()
        },

        pubgrub::PubGrubError::ErrorInShouldCancel(source) => {
            format!("Cancelled: {}", source)
        },
    }
}

/// Resolves the dependencies of the workspace members.
pub fn solve<C: CrateCatalog>(catalog: &C, options: &SolveOptions) -> Result<Solution, SolverError> {
    let mut patches: HashMap<(PatchTarget, String), Vec<Arc<Summary>>>
        = HashMap::new();

    for (target, dependency) in &options.patches {
        let summary
            = source_summary(catalog, dependency)?;

        patches.entry((target.clone(), summary.name.clone())).or_default().push(summary);
    }

    let mut replacements
        = HashMap::new();

    for replacement in &options.replacements {
        let summary
            = source_summary(catalog, &replacement.dependency)?;

        if summary.name != replacement.name || summary.version != replacement.version {
            return Err(SolverError(format!(
                "{} {} can only be replaced by a crate with the same name and version (found {} {})",
                replacement.name, replacement.version,
                summary.name, summary.version,
            )));
        }

        replacements.insert((replacement.name.clone(), replacement.version.clone()), summary);
    }

    let provider = CargoProvider {
        catalog,
        options,
        patches,
        replacements,
        registry: RefCell::new(HashMap::new()),
        discovery: RefCell::new(HashMap::new()),
    };

    // Locked crates are most likely from the default registry; the others
    // are fetched as the solver gets to them
    catalog.prefetch(&options.locked.registry.keys().map(|name| (None, name.clone())).collect::<Vec<_>>());

    let selected
        = pubgrub::resolve(&provider, SolverPackage::Root, root_version())
            .map_err(|error| SolverError(describe_failure(error)))?;

    let mut versions: BTreeMap<PackageKey, CargoSemver>
        = BTreeMap::new();
    let mut features: BTreeMap<PackageKey, BTreeSet<String>>
        = BTreeMap::new();
    let mut wide_choices: HashMap<WideRequirement, CargoBucket>
        = HashMap::new();

    for (package, version) in selected {
        match package {
            SolverPackage::Root => {},

            SolverPackage::Crate(key) => {
                versions.insert(key, version);
            },

            SolverPackage::Feature(key, value) => {
                features.entry(key).or_default().insert(value);
            },

            SolverPackage::Wide(wide) => {
                wide_choices.insert(wide, version.bucket());
            },
        }
    }

    let indices = versions.keys().enumerate()
        .map(|(index, key)| (key.clone(), index))
        .collect::<BTreeMap<_, _>>();

    let mut packages
        = Vec::with_capacity(versions.len());

    for (key, version) in &versions {
        let summary
            = provider.summary(key, version)?;

        let is_member
            = provider.is_member(key);

        let enabled
            = features.remove(key).unwrap_or_default();

        let mut edges
            = Vec::new();

        for (dependency, extra) in activated_dependencies(&summary, &enabled, is_member) {
            let target_key = match provider.edge_target(dependency, &requested_features(dependency, &extra))? {
                EdgeTarget::Single(key, _) => key,

                EdgeTarget::Wide(wide) => PackageKey::Registry {
                    bucket: wide_choices.get(&wide).cloned()
                        .ok_or_else(|| SolverError(format!("The solver didn't pick a version for {} ({})", wide.name, wide.requirement)))?,
                    registry: wide.registry,
                    name: wide.name,
                },
            };

            let target = indices.get(&target_key).copied()
                .ok_or_else(|| SolverError(format!("The solver didn't pick a version for {}, which {} depends on", target_key, key)))?;

            edges.push(SolvedEdge {
                dependency: dependency.clone(),
                target,
            });
        }

        packages.push(SolvedPackage {
            key: key.clone(),
            summary,
            is_member,
            features: enabled,
            edges,
        });
    }

    check_links(&packages)?;

    let identity
        = |summary: &Summary| (summary.name.clone(), summary.version.clone(), summary.source.clone());

    let used = packages.iter()
        .map(|package| identity(&package.summary))
        .collect::<BTreeSet<_>>();

    let mut unused_patches = provider.patches.values()
        .flatten()
        .filter(|patch| !used.contains(&identity(patch)))
        .cloned()
        .collect::<Vec<_>>();

    unused_patches.sort_by_key(|patch| identity(patch));
    unused_patches.dedup_by_key(|patch| identity(patch));

    Ok(Solution {packages, unused_patches})
}

/// A native library can only be linked once, so at most one crate of the
/// graph may declare a given `links` value.
fn check_links(packages: &[SolvedPackage]) -> Result<(), SolverError> {
    let mut owners: BTreeMap<&str, &SolvedPackage>
        = BTreeMap::new();

    for package in packages {
        let Some(links) = package.summary.links.as_deref() else {
            continue;
        };

        if let Some(owner) = owners.insert(links, package) {
            return Err(SolverError(format!(
                "Both {} {} and {} {} link the native library `{}`; only one of them can be part of the graph",
                owner.summary.name, owner.summary.version,
                package.summary.name, package.summary.version,
                links,
            )));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use zpm_primitives::CargoRequirement;
    use zpm_utils::FromFileString;

    use super::*;

    #[derive(Default)]
    struct Fixtures {
        registry: BTreeMap<String, Vec<Arc<Summary>>>,
        paths: BTreeMap<Path, Arc<Summary>>,
    }

    const INTERNAL: &str
        = "https://crates.example.org/index/";

    impl CrateCatalog for Fixtures {
        fn registry(&self, registry: &RegistryIndex, name: &str) -> Result<Arc<CrateVersions>, SolverError> {
            // The crates of the other registry are published as `internal/<name>`
            let key = match registry {
                Some(_) => format!("internal/{}", name),
                None => name.to_string(),
            };

            let mut versions = self.registry.get(&key).cloned()
                .ok_or_else(|| SolverError(format!("Unknown crate {}", key)))?;

            versions.sort_by(|a, b| a.version.cmp(&b.version));

            Ok(Arc::new(CrateVersions {versions}))
        }

        fn path(&self, dir: &Path) -> Result<Arc<Summary>, SolverError> {
            self.paths.get(dir).cloned()
                .ok_or_else(|| SolverError(format!("Unknown path {}", dir.to_file_string())))
        }

        fn git(&self, source: &CargoGitSource) -> Result<Arc<GitRepository>, SolverError> {
            Err(SolverError(format!("Unknown repository {}", source.as_str())))
        }
    }

    /// `name req [key=value...]`, with the keys `features`, `optional`,
    /// `default`, `kind` and `package`
    fn dependency(spec: &str) -> Dependency {
        let mut parts
            = spec.split_whitespace();

        let name
            = parts.next().unwrap();

        let requirement
            = CargoRequirement::from_file_string(parts.next().unwrap()).unwrap();

        let mut dependency = Dependency {
            name_in_toml: name.to_string(),
            package: name.to_string(),
            req: requirement.to_req(),
            requirement,
            features: vec![],
            default_features: true,
            optional: false,
            kind: DependencyKind::Normal,
            target: None,
            source: DependencySource::Registry(None),
        };

        for option in parts {
            match option.split_once('=').unwrap_or((option, "")) {
                ("features", features) => dependency.features = features.split(',').map(|feature| feature.to_string()).collect(),
                ("optional", _) => dependency.optional = true,
                ("default", value) => dependency.default_features = value != "false",
                ("kind", "dev") => dependency.kind = DependencyKind::Development,
                ("kind", "build") => dependency.kind = DependencyKind::Build,
                ("package", package) => dependency.package = package.to_string(),
                ("path", path) => dependency.source = DependencySource::Path(Path::from_file_string(path).unwrap()),
                ("registry", "internal") => dependency.source = DependencySource::Registry(Some(INTERNAL.to_string())),
                _ => panic!("Unknown option {}", option),
            }
        }

        dependency
    }

    fn summary(name: &str, version: &str, dependencies: &[&str], features: &[(&str, &[&str])]) -> Summary {
        let dependencies
            = dependencies.iter().map(|spec| dependency(spec)).collect::<Vec<_>>();

        let mut features = features.iter()
            .map(|(feature, values)| (feature.to_string(), values.iter().map(|value| value.to_string()).collect()))
            .collect();

        crate::cargo::add_implicit_features(&mut features, &dependencies);

        Summary {
            name: name.to_string(),
            version: CargoSemver::parse(version).unwrap(),
            source: CrateSource::Registry(None),
            dependencies,
            features,
            checksum: None,
            yanked: false,
            links: None,
            rust_version: None,
            replaced_by: None,
        }
    }

    impl Fixtures {
        fn publish(&mut self, name: &str, version: &str, dependencies: &[&str], features: &[(&str, &[&str])]) -> &mut Self {
            // `internal/<name>` publishes on the other registry
            let mut published
                = summary(name.trim_start_matches("internal/"), version, dependencies, features);

            if name.starts_with("internal/") {
                published.source = CrateSource::Registry(Some(INTERNAL.to_string()));
            }

            self.registry.entry(name.to_string()).or_default()
                .push(Arc::new(published));

            self
        }

        fn update(&mut self, name: &str, version: &str, update: impl FnOnce(&mut Summary)) -> &mut Self {
            let versions
                = self.registry.get_mut(name).unwrap();

            let index
                = versions.iter().position(|summary| summary.version.to_string() == version).unwrap();

            let mut updated
                = (*versions[index]).clone();

            update(&mut updated);
            versions[index] = Arc::new(updated);

            self
        }

        fn rust_version(&mut self, name: &str, version: &str, rust_version: RustVersion) -> &mut Self {
            self.update(name, version, |summary| summary.rust_version = Some(rust_version))
        }

        fn yank(&mut self, name: &str, version: &str) -> &mut Self {
            self.update(name, version, |summary| summary.yanked = true)
        }

        fn link(&mut self, name: &str, version: &str, links: &str) -> &mut Self {
            self.update(name, version, |summary| summary.links = Some(links.to_string()))
        }

        fn local(&mut self, dir: &str, name: &str, dependencies: &[&str], features: &[(&str, &[&str])]) -> &mut Self {
            self.local_version(dir, name, "0.1.0", dependencies, features)
        }

        fn local_version(&mut self, dir: &str, name: &str, version: &str, dependencies: &[&str], features: &[(&str, &[&str])]) -> &mut Self {
            let dir
                = Path::from_file_string(dir).unwrap();

            let mut local
                = summary(name, version, dependencies, features);

            local.source = CrateSource::Path(dir.clone());

            self.paths.insert(dir, Arc::new(local));
            self
        }

        fn solve_with(&self, members: &[&str], locked: &[(&str, &str)]) -> Result<Vec<String>, String> {
            let mut options
                = SolveOptions::default();

            for (name, version) in locked {
                options.locked.registry.entry(name.to_string()).or_default().insert(CargoSemver::parse(version).unwrap());
            }

            self.solve_options(members, options)
        }

        fn solve_options(&self, members: &[&str], mut options: SolveOptions) -> Result<Vec<String>, String> {
            options.members = members.iter()
                .map(|dir| Path::from_file_string(dir).unwrap())
                .collect();

            let solution
                = solve(self, &options).map_err(|err| err.0)?;

            let unused = solution.unused_patches.iter()
                .map(|patch| format!("unused patch: {}@{}", patch.name, patch.version));

            Ok(solution.packages.iter().map(|package| {
                let features = package.features.iter()
                    .filter(|value| matches!(FeatureValue::parse(value), FeatureValue::Feature(_)))
                    .cloned()
                    .collect::<Vec<_>>();

                let mut edges = package.edges.iter()
                    .map(|edge| {
                        let target
                            = &solution.packages[edge.target];

                        format!("{}@{}", target.summary.name, target.summary.version)
                    })
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect::<Vec<_>>();

                edges.sort();

                // `!` marks the patches, `~` what was replaced
                let marker = match (&package.summary.source, &package.key) {
                    _ if package.summary.replaced_by.is_some() => "~",
                    (CrateSource::Registry(Some(_)), _) => "+internal",
                    (CrateSource::Path(_) | CrateSource::Git {..}, PackageKey::Registry {..}) => "!",
                    _ => "",
                };

                format!("{}@{}{} [{}] -> {}", package.summary.name, package.summary.version, marker, features.join(","), edges.join(" "))
            }).chain(unused).collect())
        }

        fn patch(target: PatchTarget, spec: &str) -> (PatchTarget, Dependency) {
            (target, dependency(spec))
        }

        fn solve(&self) -> Result<Vec<String>, String> {
            self.solve_with(&["/app"], &[])
        }
    }

    #[test]
    fn test_highest_compatible_version() {
        let mut fixtures
            = Fixtures::default();

        fixtures
            .local("/app", "app", &["a ^1.0"], &[])
            .publish("a", "1.0.0", &[], &[])
            .publish("a", "1.2.0", &[], &[])
            .publish("a", "1.3.0-beta.1", &[], &[])
            .publish("a", "2.0.0", &[], &[]);

        assert_eq!(fixtures.solve().unwrap(), vec![
            "a@1.2.0 [default] -> ",
            "app@0.1.0 [] -> a@1.2.0",
        ]);
    }

    #[test]
    fn test_incompatible_versions_coexist_but_compatible_ones_are_unified() {
        let mut fixtures
            = Fixtures::default();

        fixtures
            .local("/app", "app", &["a ^1.0", "b 1", "c 1"], &[])
            .publish("a", "1.0.0", &[], &[])
            .publish("a", "1.5.0", &[], &[])
            .publish("a", "2.1.0", &[], &[])
            // `b` needs the next major, which can live next to the first
            .publish("b", "1.0.0", &["a 2"], &[])
            // `c` wants an older compatible version; a single 1.x is kept
            .publish("c", "1.0.0", &["a >=1.0,<1.5"], &[]);

        assert_eq!(fixtures.solve().unwrap(), vec![
            "a@1.0.0 [default] -> ",
            "a@2.1.0 [default] -> ",
            "b@1.0.0 [default] -> a@2.1.0",
            "c@1.0.0 [default] -> a@1.0.0",
            "app@0.1.0 [] -> a@1.0.0 b@1.0.0 c@1.0.0",
        ]);
    }

    #[test]
    fn test_conflicts_within_a_bucket_are_reported() {
        let mut fixtures
            = Fixtures::default();

        fixtures
            .local("/app", "app", &["a =1.0.0", "b 1"], &[])
            .publish("a", "1.0.0", &[], &[])
            .publish("a", "1.5.0", &[], &[])
            .publish("b", "1.0.0", &["a ^1.5"], &[]);

        let error
            = fixtures.solve().unwrap_err();

        assert!(error.contains("No solution found"), "{}", error);
    }

    #[test]
    fn test_backtracking_on_dependents() {
        let mut fixtures
            = Fixtures::default();

        fixtures
            .local("/app", "app", &["a =1.0.0", "b 1"], &[])
            .publish("a", "1.0.0", &[], &[])
            .publish("a", "1.5.0", &[], &[])
            .publish("b", "1.0.0", &["a ^1.0"], &[])
            // The latest `b` can't be used along with the pinned `a`
            .publish("b", "1.1.0", &["a ^1.5"], &[]);

        assert_eq!(fixtures.solve().unwrap(), vec![
            "a@1.0.0 [default] -> ",
            "b@1.0.0 [default] -> a@1.0.0",
            "app@0.1.0 [] -> a@1.0.0 b@1.0.0",
        ]);
    }

    #[test]
    fn test_features_enable_optional_dependencies() {
        let mut fixtures
            = Fixtures::default();

        fixtures
            .local("/app", "app", &["a 1 features=extra", "b 1 default=false"], &[])
            .publish("a", "1.0.0", &["opt 1 optional", "unused 1 optional", "weak 1 optional", "always 1"], &[
                ("default", &["std"]),
                ("std", &["always/std"]),
                ("extra", &["dep:opt", "weak?/turbo"]),
            ])
            .publish("b", "1.0.0", &["heavy 1 optional"], &[("default", &["heavy"])])
            .publish("always", "1.0.0", &[], &[("std", &[])])
            .publish("opt", "1.0.0", &[], &[])
            .publish("weak", "1.0.0", &[], &[("turbo", &[])])
            .publish("heavy", "1.0.0", &[], &[])
            .publish("unused", "1.0.0", &[], &[]);

        assert_eq!(fixtures.solve().unwrap(), vec![
            // Like in Cargo's lockfiles, a weak dependency feature does
            // bring the dependency in
            "a@1.0.0 [default,extra,std] -> always@1.0.0 opt@1.0.0 weak@1.0.0",
            // `std` is enabled through `a`'s default features
            "always@1.0.0 [default,std] -> ",
            // `default-features = false` keeps `heavy` out
            "b@1.0.0 [] -> ",
            "opt@1.0.0 [default] -> ",
            "weak@1.0.0 [default,turbo] -> ",
            "app@0.1.0 [] -> a@1.0.0 b@1.0.0",
        ]);
    }

    #[test]
    fn test_features_are_unified_across_dependents() {
        let mut fixtures
            = Fixtures::default();

        fixtures
            .local("/app", "app", &["a 1 default=false features=one", "b 1"], &[])
            .publish("a", "1.0.0", &["x 1 optional", "y 1 optional"], &[("one", &["dep:x"]), ("two", &["dep:y"])])
            .publish("b", "1.0.0", &["a 1 default=false features=two"], &[])
            .publish("x", "1.0.0", &[], &[])
            .publish("y", "1.0.0", &[], &[]);

        let solution
            = fixtures.solve().unwrap();

        assert!(solution.contains(&"a@1.0.0 [one,two] -> x@1.0.0 y@1.0.0".to_string()), "{:#?}", solution);
    }

    #[test]
    fn test_versions_lacking_a_requested_feature_are_skipped() {
        let mut fixtures
            = Fixtures::default();

        fixtures
            .local("/app", "app", &["a 1 features=legacy"], &[])
            .publish("a", "1.0.0", &[], &[("legacy", &[])])
            .publish("a", "1.1.0", &[], &[]);

        assert_eq!(fixtures.solve().unwrap()[0], "a@1.0.0 [default,legacy] -> ");
    }

    #[test]
    fn test_members_get_all_features_and_dev_dependencies() {
        let mut fixtures
            = Fixtures::default();

        fixtures
            .local("/app", "app", &["opt 1 optional", "tool 1 kind=dev", "dep 1"], &[("fancy", &["dep:opt"])])
            // Dev-dependencies of dependencies aren't part of the graph
            .publish("dep", "1.0.0", &["never 1 kind=dev", "script 1 kind=build"], &[])
            .publish("opt", "1.0.0", &[], &[])
            .publish("tool", "1.0.0", &[], &[])
            .publish("script", "1.0.0", &[], &[])
            .publish("never", "1.0.0", &[], &[]);

        assert_eq!(fixtures.solve().unwrap(), vec![
            "dep@1.0.0 [default] -> script@1.0.0",
            "opt@1.0.0 [default] -> ",
            "script@1.0.0 [default] -> ",
            "tool@1.0.0 [default] -> ",
            "app@0.1.0 [fancy] -> dep@1.0.0 opt@1.0.0 tool@1.0.0",
        ]);
    }

    #[test]
    fn test_locked_versions_are_kept_when_possible() {
        let mut fixtures
            = Fixtures::default();

        fixtures
            .local("/app", "app", &["a 1", "b 1"], &[])
            .publish("a", "1.0.0", &[], &[])
            .publish("a", "1.1.0", &[], &[])
            .publish("a", "1.2.0", &[], &[])
            .publish("b", "1.0.0", &[], &[])
            .publish("b", "1.1.0", &[], &[])
            .yank("a", "1.1.0");

        // Yanked versions are fine as long as they're locked; `b`'s lock
        // doesn't match the manifest anymore, so it gets the latest
        assert_eq!(fixtures.solve_with(&["/app"], &[("a", "1.1.0"), ("b", "0.9.0")]).unwrap(), vec![
            "a@1.1.0 [default] -> ",
            "b@1.1.0 [default] -> ",
            "app@0.1.0 [] -> a@1.1.0 b@1.1.0",
        ]);

        // ... but they're never picked otherwise
        assert_eq!(fixtures.solve().unwrap()[0], "a@1.2.0 [default] -> ");
    }

    #[test]
    fn test_requirements_spanning_several_buckets() {
        let mut fixtures
            = Fixtures::default();

        fixtures
            .local("/app", "app", &["a >=1,<4", "b 1"], &[])
            .publish("a", "1.0.0", &[], &[])
            .publish("a", "2.0.0", &[], &[])
            .publish("a", "3.0.0", &[], &[])
            .publish("b", "1.0.0", &["a 2"], &[]);

        // The wide requirement is free to take the highest bucket, while
        // `b` gets the one it asked for
        assert_eq!(fixtures.solve().unwrap(), vec![
            "a@2.0.0 [default] -> ",
            "a@3.0.0 [default] -> ",
            "b@1.0.0 [default] -> a@2.0.0",
            "app@0.1.0 [] -> a@3.0.0 b@1.0.0",
        ]);

        // ... unless another bucket was locked
        assert_eq!(fixtures.solve_with(&["/app"], &[("a", "2.0.0")]).unwrap(), vec![
            "a@2.0.0 [default] -> ",
            "b@1.0.0 [default] -> a@2.0.0",
            "app@0.1.0 [] -> a@2.0.0 b@1.0.0",
        ]);
    }

    #[test]
    fn test_renamed_dependencies_and_path_dependencies() {
        let mut fixtures
            = Fixtures::default();

        fixtures
            .local("/app", "app", &["old 1 package=a", "new 2 package=a", "lib * path=/lib"], &[])
            .local("/lib", "lib", &["a 2 features=x", "private 1 kind=dev"], &[])
            .publish("a", "1.0.0", &[], &[])
            .publish("a", "2.0.0", &[], &[("x", &[])])
            .publish("private", "1.0.0", &[], &[]);

        // `lib` isn't a member, so its dev-dependencies are left out
        assert_eq!(fixtures.solve().unwrap(), vec![
            "a@1.0.0 [default] -> ",
            "a@2.0.0 [default,x] -> ",
            "app@0.1.0 [] -> a@1.0.0 a@2.0.0 lib@0.1.0",
            "lib@0.1.0 [default] -> a@2.0.0",
        ]);
    }

    #[test]
    fn test_patches_are_candidates_among_the_others() {
        let mut fixtures
            = Fixtures::default();

        fixtures
            .local("/app", "app", &["a 1", "b 1"], &[])
            .publish("a", "1.0.0", &[], &[])
            .publish("a", "1.1.0", &["old 1"], &[])
            .publish("b", "1.0.0", &[], &[])
            .publish("b", "1.5.0", &[], &[])
            .publish("old", "1.0.0", &[], &[])
            .publish("new", "1.0.0", &[], &[])
            // Same version as the latest `a`: the fork hides it
            .local_version("/forks/a", "a", "1.1.0", &["new 1"], &[])
            // Older than the latest `b`, but patches are preferred
            .local_version("/forks/b", "b", "1.2.0", &[], &[])
            // Nothing depends on `c`, and `old` wants another major
            .local_version("/forks/c", "c", "3.0.0", &[], &[])
            .local_version("/forks/old", "old", "2.0.0", &[], &[]);

        let options = SolveOptions {
            patches: vec![
                Fixtures::patch(PatchTarget::Registry(None), "a * path=/forks/a"),
                Fixtures::patch(PatchTarget::Registry(None), "b * path=/forks/b"),
                Fixtures::patch(PatchTarget::Registry(None), "c * path=/forks/c"),
                Fixtures::patch(PatchTarget::Registry(None), "old * path=/forks/old"),
            ],
            ..Default::default()
        };

        assert_eq!(fixtures.solve_options(&["/app"], options.clone()).unwrap(), vec![
            "a@1.1.0! [default] -> new@1.0.0",
            "b@1.2.0! [default] -> ",
            "new@1.0.0 [default] -> ",
            "app@0.1.0 [] -> a@1.1.0 b@1.2.0",
            "unused patch: c@3.0.0",
            "unused patch: old@2.0.0",
        ]);

        // A lockfile holding a published version doesn't switch to an
        // older patch by itself (that's what `yarn up -R` is for)
        let mut locked
            = options;

        locked.locked.registry.insert("b".to_string(), BTreeSet::from([CargoSemver::parse("1.5.0").unwrap()]));

        let solution
            = fixtures.solve_options(&["/app"], locked).unwrap();

        assert!(solution.contains(&"b@1.5.0 [default] -> ".to_string()), "{:#?}", solution);
        assert!(solution.contains(&"unused patch: b@1.2.0".to_string()), "{:#?}", solution);
    }

    #[test]
    fn test_replacements_keep_the_identity_of_what_they_replace() {
        let mut fixtures
            = Fixtures::default();

        fixtures
            .local("/app", "app", &["a 1 features=extra"], &[])
            .publish("a", "1.0.0", &["old 1"], &[("extra", &[])])
            .publish("old", "1.0.0", &[], &[])
            .publish("new", "1.0.0", &[], &[])
            .local_version("/forks/a", "a", "1.0.0", &["new 1"], &[("extra", &[])])
            .local_version("/forks/other", "a", "1.0.1", &[], &[]);

        let replace = |path: &str| SolveOptions {
            replacements: vec![Replacement {
                name: "a".to_string(),
                version: CargoSemver::parse("1.0.0").unwrap(),
                dependency: dependency(&format!("a * path={}", path)),
            }],
            ..Default::default()
        };

        // The registry crate stays in the graph, with the dependencies
        // and features of what replaces it
        assert_eq!(fixtures.solve_options(&["/app"], replace("/forks/a")).unwrap(), vec![
            "a@1.0.0~ [default,extra] -> new@1.0.0",
            "new@1.0.0 [default] -> ",
            "app@0.1.0 [] -> a@1.0.0",
        ]);

        let error
            = fixtures.solve_options(&["/app"], replace("/forks/other")).unwrap_err();

        assert!(error.contains("same name and version"), "{}", error);
    }

    #[test]
    fn test_versions_supporting_the_rust_version_are_preferred() {
        let mut fixtures
            = Fixtures::default();

        fixtures
            .local("/app", "app", &["a 1", "b 1", "c 1"], &[])
            .publish("a", "1.0.0", &[], &[])
            .publish("a", "1.1.0", &[], &[])
            .publish("a", "1.2.0", &[], &[])
            .rust_version("a", "1.0.0", (1, 60, 0))
            .rust_version("a", "1.1.0", (1, 70, 0))
            .rust_version("a", "1.2.0", (1, 80, 0))
            // Nothing supports the workspace: the latest is as good as any
            .publish("b", "1.0.0", &[], &[])
            .publish("b", "1.1.0", &[], &[])
            .rust_version("b", "1.0.0", (1, 90, 0))
            .rust_version("b", "1.1.0", (1, 91, 0))
            // Crates that don't say are assumed to be fine
            .publish("c", "1.0.0", &[], &[])
            .publish("c", "1.1.0", &[], &[])
            // The preference applies across majors too
            .publish("d", "1.0.0", &["wide >=1,<3"], &[])
            .publish("wide", "1.4.0", &[], &[])
            .publish("wide", "2.0.0", &[], &[])
            .rust_version("wide", "1.4.0", (1, 70, 0))
            .rust_version("wide", "2.0.0", (1, 85, 0));

        fixtures.paths.clear();
        fixtures.local("/app", "app", &["a 1", "b 1", "c 1", "d 1"], &[]);

        let options = SolveOptions {
            rust_versions: vec![(1, 75, 0)],
            ..Default::default()
        };

        assert_eq!(fixtures.solve_options(&["/app"], options).unwrap(), vec![
            "a@1.1.0 [default] -> ",
            "b@1.1.0 [default] -> ",
            "c@1.1.0 [default] -> ",
            "d@1.0.0 [default] -> wide@1.4.0",
            "wide@1.4.0 [default] -> ",
            "app@0.1.0 [] -> a@1.1.0 b@1.1.0 c@1.1.0 d@1.0.0",
        ]);

        assert_eq!(fixtures.solve().unwrap()[0], "a@1.2.0 [default] -> ");
    }

    #[test]
    fn test_registries_are_distinct_namespaces() {
        let mut fixtures
            = Fixtures::default();

        fixtures
            .local("/app", "app", &["a 1", "private 1 package=a registry=internal"], &[])
            .publish("a", "1.0.0", &[], &[])
            // Dependencies of a crate come from its own registry, unless
            // they say otherwise
            .publish("internal/a", "1.4.0", &["shared 1 registry=internal"], &[])
            .publish("internal/shared", "1.0.0", &[], &[]);

        assert_eq!(fixtures.solve().unwrap(), vec![
            "a@1.0.0 [default] -> ",
            "a@1.4.0+internal [default] -> shared@1.0.0",
            "shared@1.0.0+internal [default] -> ",
            "app@0.1.0 [] -> a@1.0.0 a@1.4.0",
        ]);
    }

    #[test]
    fn test_native_libraries_are_linked_once() {
        let mut fixtures
            = Fixtures::default();

        fixtures
            .local("/app", "app", &["sys 1", "other 1"], &[])
            .publish("sys", "1.0.0", &[], &[])
            .publish("other", "1.0.0", &["sys 2"], &[])
            .publish("sys", "2.0.0", &[], &[])
            .link("sys", "1.0.0", "z")
            .link("sys", "2.0.0", "z");

        let error
            = fixtures.solve().unwrap_err();

        assert!(error.contains("link the native library `z`"), "{}", error);
    }
}
