use std::{collections::{BTreeMap, BTreeSet}, fmt::{self, Debug, Display}, hash::Hash, marker::PhantomData, sync::{Arc, Mutex}};

use rkyv::Archive;
use serde::{de::{self, Visitor}, Deserialize, Deserializer, Serialize, Serializer};
use serde_with::{serde_as, DefaultOnError};
use zpm_config::Configuration;
use zpm_parsers::JsonDocument;
use zpm_primitives::{Descriptor, Ident, Locator, PeerRange, Range, Reference, RegistryReference, RegistrySemverRange, SemverDescriptor};
use zpm_utils::{FromFileString, Hash64, Hash64Writer, Path, ToFileString, UrlEncoded};

use crate::{
    error::Error, http_npm, install::{DependencyNormalizer, InstallContext, RuleUsage, normalize_resolutions_with}, manifest::resolutions::ResolutionsField, npm, primitives_exts::RangeExt, project::Project, resolvers::Resolution
};

const LOCKFILE_VERSION: u64 = 9;

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LockfilePeerDependencyMeta {
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub optional: Option<bool>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LockfilePackageExtension {
    #[serde(default)]
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub dependencies: BTreeMap<Ident, Range>,

    #[serde(default)]
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub peer_dependencies: BTreeMap<Ident, PeerRange>,

    #[serde(default)]
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub peer_dependencies_meta: BTreeMap<Ident, LockfilePeerDependencyMeta>,
}

impl LockfilePackageExtension {
    pub fn from_config(extension: &zpm_config::PackageExtension) -> Self {
        Self {
            dependencies: extension.dependencies.iter()
                .map(|(ident, range)| (ident.clone(), range.value.clone()))
                .collect(),
            peer_dependencies: extension.peer_dependencies.iter()
                .map(|(ident, range)| (ident.clone(), range.value.clone()))
                .collect(),
            peer_dependencies_meta: extension.peer_dependencies_meta.iter()
                .map(|(ident, meta)| (ident.clone(), LockfilePeerDependencyMeta {optional: meta.optional.value}))
                .collect(),
        }
    }
}

pub type LockfileCatalogs = BTreeMap<String, BTreeMap<Ident, Range>>;

pub fn catalogs_from_config(catalogs: &BTreeMap<String, BTreeMap<Ident, zpm_config::Setting<Range>>>) -> LockfileCatalogs {
    catalogs.iter()
        .map(|(name, catalog)| {
            let entries = catalog.iter()
                .map(|(ident, range)| (ident.clone(), range.value.clone()))
                .collect();

            (name.clone(), entries)
        })
        .collect()
}

/**
 * Project-level information that, together with the lockfile entries, is
 * enough to reconstruct the dependency tree without having to look at the
 * project the lockfile was generated from (which may not be around anymore,
 * typically when the lockfile is read from a past commit).
 */
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LockfileProject {
    /**
     * Hash of the normalized dependencies of each workspace. It only covers
     * the dependencies the workspace itself declares, not their own
     * dependencies; those must be obtained by walking the lockfile.
     */
    #[serde(default)]
    pub workspaces: BTreeMap<Ident, Hash64>,

    /**
     * The entries from the `catalog` and `catalogs` settings that are
     * referenced by the project; the former is stored under the `default`
     * key, just like it is in the configuration.
     */
    #[serde(default)]
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub catalogs: LockfileCatalogs,

    /**
     * The entries from the `resolutions` field of the root manifest that
     * apply to at least one dependency.
     */
    #[serde(default)]
    #[serde(skip_serializing_if = "ResolutionsField::is_empty")]
    pub dependency_overrides: ResolutionsField,

    /**
     * The entries from the `packageExtensions` setting that match at least
     * one package.
     */
    #[serde(default)]
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub package_extensions: BTreeMap<SemverDescriptor, LockfilePackageExtension>,
}

impl LockfileProject {
    /**
     * Snapshots the rules the given project applies on its dependency tree.
     * We only keep those that have an actual effect on the given packages, so
     * that the lockfile doesn't change when unrelated settings are modified.
     */
    pub fn from_project<'a>(project: &Project, resolutions: impl IntoIterator<Item = &'a Resolution>) -> Result<Self, Error> {
        let rule_usage
            = Arc::new(Mutex::new(RuleUsage::default()));

        let context
            = InstallContext::default()
                .with_project(Some(project))
                .with_rule_usage(Some(rule_usage.clone()));

        let mut workspaces
            = BTreeMap::new();

        for (ident, dependencies) in project.workspace_dependencies_with(&context) {
            workspaces.insert(ident, hash_workspace_dependencies(&dependencies?));
        }

        let all_overrides
            = &project.root_workspace().manifest.resolutions;

        // Finding out which rules are used requires to normalize all the
        // packages once more; no need to pay for it if there's no rule.
        let has_rules
            = !all_overrides.is_empty()
                || !context.package_extensions.is_empty()
                || context.catalogs.values().any(|catalog| !catalog.is_empty());

        if has_rules {
            let normalizer
                = DependencyNormalizer::from_context(&context);

            // The workspaces have already been accounted for when we retrieved their dependencies
            let package_resolutions = resolutions.into_iter()
                .filter(|resolution| !resolution.locator.reference.is_workspace_reference());

            for resolution in package_resolutions {
                normalize_resolutions_with(&normalizer, resolution)?;
            }
        }

        let rule_usage
            = rule_usage.lock().unwrap();

        let mut catalogs
            = LockfileCatalogs::new();

        for (catalog_name, idents) in &rule_usage.catalog_entries {
            let entries = idents.iter()
                .filter_map(|ident| Some((ident.clone(), context.catalogs.get(catalog_name)?.get(ident)?.clone())))
                .collect();

            catalogs.insert(catalog_name.clone(), entries);
        }

        // Two entries can share a selector (`**/foo` and `foo` both parse
        // into the same one), in which case only the first can ever match.
        // Keeping both would write the same JSON key twice, and the value
        // any other tool would read back is the one we never applied.
        let mut stored_selectors
            = BTreeSet::new();

        let dependency_overrides = all_overrides.iter()
            .filter(|(selector, _)| rule_usage.dependency_overrides.contains(selector))
            .filter(|(selector, _)| stored_selectors.insert((*selector).clone()))
            .map(|(selector, range)| (selector.clone(), range.clone()));

        let package_extensions = context.package_extensions.iter()
            .filter(|(descriptor, _)| rule_usage.package_extensions.contains(descriptor))
            .map(|(descriptor, extension)| (descriptor.clone(), extension.clone()))
            .collect();

        Ok(Self {
            workspaces,
            catalogs,
            dependency_overrides: ResolutionsField::from_entries(dependency_overrides),
            package_extensions,
        })
    }
}

/**
 * Hashes the dependencies of a workspace. They're expected to be normalized
 * (ie. to be the descriptors that get resolved rather than the ones found
 * in the manifest), so that the hash changes should a catalog be updated.
 */
pub fn hash_workspace_dependencies(dependencies: &BTreeMap<Ident, Descriptor>) -> Hash64 {
    let mut writer
        = Hash64Writer::new();

    for descriptor in dependencies.values() {
        writer.update(descriptor.to_file_string());
        writer.update([0]);
    }

    writer.finalize()
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Archive, rkyv::Serialize, rkyv::Deserialize)]
#[rkyv(serialize_bounds(__S: rkyv::ser::Writer + rkyv::ser::Allocator + rkyv::ser::Sharing, <__S as rkyv::rancor::Fallible>::Error: rkyv::rancor::Source))]
#[rkyv(deserialize_bounds(__D: rkyv::de::Pooling, <__D as rkyv::rancor::Fallible>::Error: rkyv::rancor::Source))]
#[rkyv(bytecheck(bounds(__C: rkyv::validation::ArchiveContext + rkyv::validation::SharedContext, <__C as rkyv::rancor::Fallible>::Error: rkyv::rancor::Source)))]
pub struct LockfileEntry {
    pub checksum: Option<Hash64>,
    pub resolution: Resolution,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Lockfile {
    pub metadata: LockfileMetadata,
    pub project: LockfileProject,
    pub resolutions: BTreeMap<Descriptor, Locator>,
    pub entries: BTreeMap<Locator, LockfileEntry>,

    /**
     * Transient resolutions are recomputed by every install, so they're kept
     * out of the regular resolution tables to guarantee that nothing ever
     * reuses them. They're only stored in the lockfile so that the dependency
     * tree can be reconstructed from the lockfile alone.
     */
    pub transient_resolutions: BTreeMap<Descriptor, Locator>,
    pub transient_entries: BTreeMap<Locator, LockfileEntry>,

    pub islands: BTreeMap<String, BTreeMap<Descriptor, Locator>>,
}

impl Lockfile {
    pub fn new() -> Self {
        Self {
            metadata: LockfileMetadata::new(),
            project: LockfileProject::default(),
            resolutions: BTreeMap::new(),
            entries: BTreeMap::new(),
            transient_resolutions: BTreeMap::new(),
            transient_entries: BTreeMap::new(),
            islands: BTreeMap::new(),
        }
    }

    /**
     * Returns the locator a descriptor resolved to when the lockfile was
     * generated, regardless of whether an install would reuse it or not.
     */
    pub fn recorded_resolution(&self, descriptor: &Descriptor) -> Option<&Locator> {
        self.resolutions.get(descriptor)
            .or_else(|| self.transient_resolutions.get(descriptor))
    }

    pub fn recorded_entry(&self, locator: &Locator) -> Option<&LockfileEntry> {
        self.entries.get(locator)
            .or_else(|| self.transient_entries.get(locator))
    }
}

impl<'de> Deserialize<'de> for Lockfile {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> where D: Deserializer<'de> {
        let payload = LockfilePayload::deserialize(deserializer)?;

        let mut lockfile = Lockfile::new();

        lockfile.metadata = payload.metadata;
        lockfile.project = payload.project;

        for (key, entry) in payload.entries {
            // Workspaces are always resolved from the project itself. We don't
            // write them in the lockfile, but older versions used to when a
            // registry range happened to be fulfilled by a workspace.
            if entry.resolution.locator.reference.is_workspace_reference() {
                continue;
            }

            let (transient_descriptors, descriptors): (Vec<_>, Vec<_>)
                = key.0.into_iter()
                    .partition(|descriptor| descriptor.range.details().transient_resolution);

            if !transient_descriptors.is_empty() {
                for descriptor in transient_descriptors {
                    lockfile.transient_resolutions.insert(descriptor, entry.resolution.locator.clone());
                }

                lockfile.transient_entries.insert(entry.resolution.locator.clone(), entry.clone());
            }

            if !descriptors.is_empty() {
                for descriptor in descriptors {
                    lockfile.resolutions.insert(descriptor, entry.resolution.locator.clone());
                }

                lockfile.entries.insert(entry.resolution.locator.clone(), entry);
            }
        }

        // Deserialize island entries
        for (island_id, island_entries) in payload.islands {
            let mut island_resolutions = BTreeMap::new();
            for (key, entry) in island_entries {
                for descriptor in key.0 {
                    island_resolutions.insert(descriptor, entry.resolution.locator.clone());
                }
                lockfile.entries.insert(entry.resolution.locator.clone(), entry);
            }
            lockfile.islands.insert(island_id, island_resolutions);
        }

        Ok(lockfile)
    }
}

impl Serialize for Lockfile {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where S: Serializer {
        struct MultiKeyLockfileEntry {
            key: MultiKey<Descriptor>,
            inner: LockfileEntry,
        }

        let recorded_resolutions: BTreeMap<&Descriptor, &Locator>
            = self.transient_resolutions.iter()
                .chain(self.resolutions.iter())
                .collect();

        let mut descriptors_to_resolutions: BTreeMap<Locator, MultiKeyLockfileEntry> = BTreeMap::new();
        for (descriptor, locator) in recorded_resolutions {
            // Workspaces are always resolved from the project itself; we
            // only keep track of the hash of their dependencies.
            if locator.reference.is_workspace_reference() {
                continue;
            }

            let entry = self.recorded_entry(locator)
                .expect("Expected a matching resolution to be found in the lockfile for any resolved locator.");

            descriptors_to_resolutions.entry(entry.resolution.locator.clone())
                .or_insert_with(|| MultiKeyLockfileEntry {inner: entry.clone(), key: MultiKey::new()})
                .key.0
                .push(descriptor.clone());
        }

        let mut entries = BTreeMap::new();
        for mut entry in descriptors_to_resolutions.into_values() {
            // The checksum of a transient package is the hash of an archive
            // we generate locally, so it depends on the working tree rather
            // than on anything the lockfile pins. Nothing ever reads it back
            // either: keys that are entirely transient are hydrated into the
            // side tables, which installs never look at. Keys shared with a
            // regular descriptor (an alias and its inner package) keep theirs.
            if entry.key.0.iter().all(|descriptor| descriptor.range.details().transient_resolution) {
                entry.inner.checksum = None;
            }

            entries.insert(entry.key, entry.inner);
        }

        // Serialize island entries
        let mut islands_payload: BTreeMap<String, BTreeMap<MultiKey<Descriptor>, LockfileEntry>> = BTreeMap::new();
        for (island_id, island_resolutions) in &self.islands {
            let mut island_entries_map: BTreeMap<Locator, MultiKeyLockfileEntry> = BTreeMap::new();
            for (descriptor, locator) in island_resolutions {
                if descriptor.range.details().transient_resolution {
                    continue;
                }

                let entry = self.entries.get(locator)
                    .expect("Expected a matching resolution to be found in the lockfile for any resolved island locator.");

                island_entries_map.entry(entry.resolution.locator.clone())
                    .or_insert_with(|| MultiKeyLockfileEntry {inner: entry.clone(), key: MultiKey::new()})
                    .key.0
                    .push(descriptor.clone());
            }

            let mut island_entries = BTreeMap::new();
            for entry in island_entries_map.into_values() {
                island_entries.insert(entry.key, entry.inner);
            }
            islands_payload.insert(island_id.clone(), island_entries);
        }

        let payload = LockfilePayload {
            metadata: self.metadata.clone(),
            project: self.project.clone(),
            entries,
            islands: islands_payload,
        };

        payload.serialize(serializer)
    }
}

#[derive(Clone, Debug)]
struct TolerantMap<K, V>(BTreeMap<K, V>);

impl<'de, K, V> Deserialize<'de> for TolerantMap<K, V> where K: Debug + Eq + Ord + Deserialize<'de>, V: Debug + Deserialize<'de> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> where D: Deserializer<'de> {
        struct MapVisitor<K, V> {
            marker: PhantomData<fn() -> TolerantMap<K, V>>,
        }

        impl<'de, K, V> Visitor<'de> for MapVisitor<K, V> where K: Debug + Eq + Ord + Deserialize<'de>, V: Debug + Deserialize<'de> {
            type Value = TolerantMap<K, V>;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a map")
            }

            fn visit_map<A>(self, mut map: A) -> Result<TolerantMap<K, V>, A::Error> where A: de::MapAccess<'de> {
                let mut values = BTreeMap::new();

                loop {
                    let entry = map.next_entry::<K, V>();

                    if let Ok(val) = entry {
                        if let Some((key, value)) = val {
                            values.insert(key, value);
                        } else {
                            break;
                        }
                    }
                }

                Ok(TolerantMap(values))
            }
        }

        let visitor = MapVisitor {
            marker: PhantomData
        };

        deserializer.deserialize_map(visitor)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash, PartialOrd, Ord)]
struct MultiKey<T>(Vec<T>);

impl<T> MultiKey<T> {
    fn new() -> Self {
        MultiKey(vec![])
    }
}

impl<T> Serialize for MultiKey<T> where T: ToFileString {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where S: Serializer {
        let mut string = String::new();

        for (index, item) in self.0.iter().enumerate() {
            if index > 0 {
                string.push_str(", ");
            }

            let serialized = item.to_file_string();
            for ch in serialized.chars() {
                if ch == ',' || ch == '\\' {
                    string.push('\\');
                }

                string.push(ch);
            }
        }

        serializer.serialize_str(&string)
    }
}

impl<'de, T: FromFileString> Deserialize<'de> for MultiKey<T> where <T as FromFileString>::Error: Display {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> where D: Deserializer<'de> {
        struct VecVisitor<T> {
            marker: PhantomData<fn() -> T>,
        }

        impl<T: FromFileString> Visitor<'_> for VecVisitor<T> where <T as FromFileString>::Error: Display {
            type Value = Vec<T>;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a string of comma-separated values")
            }

            fn visit_str<E>(self, value: &str) -> Result<Vec<T>, E> where E: de::Error {
                let mut chunks = Vec::new();
                let mut current = String::new();
                let mut chars = value.chars().peekable();

                while let Some(ch) = chars.next() {
                    if ch == ',' {
                        chunks.push(current);
                        current = String::new();
                        continue;
                    }

                    if ch != '\\' {
                        current.push(ch);
                        continue;
                    }

                    match chars.peek() {
                        Some(',') | Some('\\') => {
                            current.push(chars.next().expect("peeked character should be present"));
                        }
                        _ => {
                            // Keep unknown escapes as-is to stay backward-compatible with legacy lockfiles.
                            current.push('\\');
                        }
                    }
                }

                chunks.push(current);

                let result = chunks.into_iter()
                    .map(|s| T::from_file_string(s.trim()))
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(de::Error::custom)?;

                Ok(result)
            }
        }

        let visitor
            = VecVisitor { marker: PhantomData };

        deserializer.deserialize_str(visitor).map(MultiKey)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct LockfileMetadata {
    pub version: u64,
}

impl LockfileMetadata {
    pub fn new() -> Self {
        let version
            = std::env::var("YARN_LOCKFILE_VERSION_OVERRIDE")
                .ok()
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(LOCKFILE_VERSION);

        LockfileMetadata {
            version,
        }
    }
}

impl Default for LockfileMetadata {
    fn default() -> Self {
        LockfileMetadata::new()
    }
}

#[serde_as]
#[derive(Deserialize, Serialize)]
struct LockfilePayload {
    #[serde(rename = "__metadata")]
    #[serde(default)]
    metadata: LockfileMetadata,

    // The project section is only informative; should we fail to make sense
    // of it, the next install will regenerate it anyway.
    #[serde(default)]
    #[serde_as(deserialize_as = "DefaultOnError")]
    project: LockfileProject,

    #[serde(default)]
    entries: BTreeMap<MultiKey<Descriptor>, LockfileEntry>,

    #[serde(default)]
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    islands: BTreeMap<String, BTreeMap<MultiKey<Descriptor>, LockfileEntry>>,
}

#[derive(Debug, Deserialize)]
struct LegacyBerryLockfileEntry {
    resolution: Locator,
}

#[derive(Deserialize)]
struct LegacyBerryLockfilePayload {
    #[serde(rename = "__metadata")]
    _metadata: serde_yaml::Value,

    #[serde(flatten)]
    entries: TolerantMap<MultiKey<Descriptor>, LegacyBerryLockfileEntry>,
}

pub fn from_legacy_berry_lockfile(data: &str) -> Result<Lockfile, Error> {
    if data.starts_with("# THIS IS AN AUTOGENERATED FILE. DO NOT EDIT THIS FILE DIRECTLY.") {
        return Err(Error::LockfileV1Error);
    }

    let payload: LegacyBerryLockfilePayload = serde_yaml::from_str(data)
        .map_err(|err| Error::LegacyLockfileParseError(Arc::new(err)))?;

    let mut lockfile
        = Lockfile::new();

    lockfile.metadata.version = 1;

    for (key, entry) in payload.entries.0 {
        let (same_idents, aliased_idents): (Vec<_>, Vec<_>)
            = key.0.into_iter()
                .partition(|descriptor| descriptor.ident == entry.resolution.ident);

        if !same_idents.is_empty() {
            lockfile.entries.insert(entry.resolution.clone(), LockfileEntry {
                checksum: None,
                resolution: Resolution::new_empty(entry.resolution.clone(), Default::default()),
            });

            for descriptor in same_idents {
                lockfile.resolutions.insert(descriptor, entry.resolution.clone());
            }
        }

        if !aliased_idents.is_empty() {
            let Reference::Registry(params) = entry.resolution.reference.clone() else {
                continue;
            };

            for descriptor in aliased_idents {
                let aliased_locator
                    = Locator::new(descriptor.ident.clone(), RegistryReference {
                        ident: entry.resolution.ident.clone(),
                        version: params.version.clone(),
                        url: params.url.clone(),
                    }.into());

                lockfile.entries.insert(entry.resolution.clone(), LockfileEntry {
                    checksum: None,
                    resolution: Resolution::new_empty(aliased_locator, Default::default()),
                });

                lockfile.resolutions.insert(descriptor, entry.resolution.clone());
            }
        }
    }

    Ok(lockfile)
}

/// Dependency entry from pnpm list --json output
#[derive(Debug, Deserialize, Clone)]
struct PnpmListDependency {
    #[serde(default)]
    version: Option<String>,

    #[serde(default)]
    resolved: Option<String>,

    #[serde(default)]
    path: Option<String>,

    #[serde(default)]
    dependencies: BTreeMap<String, PnpmListDependency>,
}

/// Builds a lockfile from pnpm's installed packages using `pnpm list --json`.
///
/// The approach:
/// 1. Run `pnpm list --json` to get the full dependency tree
///    (uses `--depth=Infinity` on pnpm >= 10.29.3, `--depth=3` on older versions)
/// 2. Recursively walk the tree to collect all packages with their resolved URLs
/// 3. For each package, read its package.json to get the original dependency ranges
/// 4. Build descriptor -> locator mappings
pub fn from_pnpm_node_modules(project_cwd: &Path, config: &Configuration) -> Result<Lockfile, Error> {
    let pnpm_dir
        = project_cwd
            .with_join_str("node_modules/.pnpm");

    if !pnpm_dir.fs_exists() {
        return Ok(Lockfile::new());
    }

    let depth_flag = std::process::Command::new("pnpm")
        .arg("--version")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|v| zpm_semver::Version::from_file_string(v.trim()).ok())
        .filter(|v| *v >= zpm_semver::Version::from_file_string("10.29.3").unwrap())
        .map_or("--depth=3", |_| "--depth=Infinity");

    let output = std::process::Command::new("pnpm")
        .args(["list", "-r", "--json", depth_flag])
        .current_dir(project_cwd.as_str())
        .output()
        .map_err(|_| Error::PnpmNodeModulesReadError)?;

    if !output.status.success() {
        return Err(Error::PnpmNodeModulesReadError);
    }

    let json_output
        = String::from_utf8_lossy(&output.stdout);

    let mut pnpm_list: Vec<PnpmListDependency>
        = JsonDocument::hydrate_from_str(&json_output)
            .map_err(|_| Error::PnpmNodeModulesReadError)?;

    let mut lockfile = Lockfile::new();
    lockfile.metadata.version = 1;

    while let Some(entry) = pnpm_list.pop() {
        pnpm_list.extend(entry.dependencies.values().cloned());

        let Some(package_path_str) = &entry.path else {
            continue;
        };

        let Ok(raw_path) = Path::try_from(package_path_str.as_str()) else {
            continue;
        };

        let package_path = if raw_path.is_relative() {
            project_cwd.with_join_str("node_modules").with_join(&raw_path)
        } else {
            raw_path
        };

        #[derive(Debug, Deserialize)]
        struct Manifest {
            #[serde(default)]
            dependencies: BTreeMap<String, String>,
            #[serde(default, rename = "optionalDependencies")]
            optional_dependencies: BTreeMap<String, String>,
        }

        let manifest: Option<Manifest>
            = package_path
                .with_join_str("package.json")
                .fs_read_text()
                .ok()
                .and_then(|content| JsonDocument::hydrate_from_str(&content).ok());

        let Some(manifest) = manifest else {
            continue;
        };

        let all_dependencies
            = manifest.dependencies.into_iter().map(|(n, r)| (n, r, false))
                .chain(manifest.optional_dependencies.into_iter().map(|(n, r)| (n, r, true)));

        for (name, range, is_optional) in all_dependencies {
            let Ok(ident) = Ident::from_file_string(&name) else {
                continue;
            };

            // We only support importing raw semver ranges for now
            let Ok(range) = zpm_semver::Range::from_file_string(&range) else {
                continue;
            };

            let Some(resolved_entry) = entry.dependencies.get(&name) else {
                continue;
            };

            if is_optional {
                let installed = resolved_entry.path.as_ref().and_then(|p| {
                    let p
                        = Path::try_from(p.as_str()).ok()?;

                    let p = if p.is_relative() {
                        project_cwd.with_join_str("node_modules").with_join(&p)
                    } else {
                        p
                    };

                    Some(p.with_join_str("package.json").fs_exists())
                });

                if !installed.unwrap_or(false) {
                    continue;
                }
            }

            let Some(version) = &resolved_entry.version else {
                continue;
            };

            // All semver ranges are assumed to resolve to a registry package, so they should have a `resolved`
            // field pointing to a .tgz url.
            let Some(resolved_field) = &resolved_entry.resolved else {
                continue;
            };

            let descriptor = Descriptor::new(ident.clone(), Range::RegistrySemver(RegistrySemverRange {
                ident: None,
                range,
            }));

            let Ok(version) = zpm_semver::Version::from_file_string(version.as_str()) else {
                continue;
            };

            let registry_base
                = http_npm::get_registry_for_ident(&config, Some(&ident), false)?;

            // Store the tarball URL only if it's non-conventional (can't be computed from registry + path)
            let url = if npm::is_conventional_tarball_url(&registry_base, &ident, &version, resolved_field.clone()) {
                None
            } else {
                Some(UrlEncoded::new(resolved_field.clone()))
            };

            let locator = Locator::new(ident.clone(), RegistryReference {
                ident: ident,
                version,
                url,
            }.into());

            lockfile.entries.insert(locator.clone(), LockfileEntry {
                checksum: None,
                resolution: Resolution::new_empty(locator.clone(), Default::default()),
            });

            lockfile.resolutions.insert(descriptor, locator);
        }
    }

    Ok(lockfile)
}

#[cfg(test)]
mod tests {
    use zpm_parsers::JsonDocument;
    use zpm_primitives::{Descriptor, Ident, Locator};
    use zpm_utils::FromFileString;

    use super::Lockfile;

    fn descriptor(src: &str) -> Descriptor {
        Descriptor::from_file_string(src).unwrap()
    }

    fn locator(src: &str) -> Locator {
        Locator::from_file_string(src).unwrap()
    }

    const LOCKFILE: &str = r#"{
  "__metadata": {
    "version": 9
  },
  "project": {
    "workspaces": {
      "root": "786a02f742015903c6c6fd852552d272912f4740e15847618a86e217f71f5419d25e1031afee585313896444934eb04b903a685b1448b755d56f701afe9be2ce"
    },
    "catalogs": {
      "default": {
        "bar": "npm:^2.0.0"
      },
      "legacy": {
        "bar": "npm:^1.0.0"
      }
    },
    "dependencyOverrides": {
      "foo/bar": "catalog:legacy",
      "bar": "catalog:",
      "qux@^1.0.0": "npm:1.2.3"
    },
    "packageExtensions": {
      "foo@*": {
        "dependencies": {
          "bar": "^1.0.0"
        },
        "peerDependenciesMeta": {
          "baz": {
            "optional": true
          }
        }
      }
    }
  },
  "entries": {
    "foo@npm:^1.0.0, foo-alias@npm:foo@^1.0.0": {
      "checksum": null,
      "resolution": {
        "resolution": "foo@npm:1.0.0",
        "version": "1.0.0"
      }
    },
    "linked@link:./linked::parent=root@workspace:root": {
      "checksum": null,
      "resolution": {
        "resolution": "linked@link:./linked::parent=root@workspace:root",
        "version": "0.0.0"
      }
    },
    "typescript@npm:^5.0.0": {
      "checksum": null,
      "resolution": {
        "resolution": "typescript@npm:5.9.3",
        "version": "5.9.3"
      }
    },
    "typescript@patch:typescript%40npm%3A%5E5.0.0#<builtin>": {
      "checksum": null,
      "resolution": {
        "resolution": "typescript@patch:typescript%40npm%3A5.9.3#<builtin>&checksum=85eaa72caadee6a5622c928b1473f16d3507770cd417f35e56c48bcc9b50a1d71dfd49ad5a227767d79fdf331a578e26ef8045e83e3f7356f72a1412ae2be199",
        "version": "5.9.3"
      }
    }
  }
}"#;

    #[test]
    fn should_keep_transient_resolutions_out_of_the_resolution_tables() {
        let lockfile: Lockfile
            = JsonDocument::hydrate_from_str(LOCKFILE).unwrap();

        assert_eq!(lockfile.resolutions.keys().cloned().collect::<Vec<_>>(), vec![
            descriptor("foo@npm:^1.0.0"),
            descriptor("typescript@npm:^5.0.0"),
        ]);

        assert_eq!(lockfile.entries.keys().cloned().collect::<Vec<_>>(), vec![
            locator("foo@npm:1.0.0"),
            locator("typescript@npm:5.9.3"),
        ]);

        assert_eq!(lockfile.transient_resolutions.keys().cloned().collect::<Vec<_>>(), vec![
            descriptor("foo-alias@npm:foo@^1.0.0"),
            descriptor("linked@link:./linked::parent=root@workspace:root"),
            descriptor("typescript@patch:typescript%40npm%3A%5E5.0.0#<builtin>"),
        ]);

        // Transient resolutions remain available to those who explicitly ask for them
        assert_eq!(
            lockfile.recorded_resolution(&descriptor("foo-alias@npm:foo@^1.0.0")),
            Some(&locator("foo@npm:1.0.0")),
        );

        assert!(lockfile.recorded_entry(&locator("linked@link:./linked::parent=root@workspace:root")).is_some());
    }

    #[test]
    fn should_be_stable_once_serialized_again() {
        let lockfile: Lockfile
            = JsonDocument::hydrate_from_str(LOCKFILE).unwrap();

        assert_eq!(JsonDocument::to_string_pretty(&lockfile).unwrap(), LOCKFILE);
    }

    #[test]
    fn should_preserve_the_dependency_overrides_order() {
        let lockfile: Lockfile
            = JsonDocument::hydrate_from_str(LOCKFILE).unwrap();

        // The first matching override wins, so the order matters
        let selectors = lockfile.project.dependency_overrides.iter()
            .map(|(selector, _)| zpm_utils::ToFileString::to_file_string(selector))
            .collect::<Vec<_>>();

        assert_eq!(selectors, vec!["foo/bar", "bar", "qux@^1.0.0"]);

        // The overrides are stored verbatim; the catalogs they reference are kept on the side
        let ranges = lockfile.project.dependency_overrides.iter()
            .map(|(_, range)| zpm_utils::ToFileString::to_file_string(range))
            .collect::<Vec<_>>();

        assert_eq!(ranges, vec!["catalog:legacy", "catalog:", "npm:1.2.3"]);

        assert_eq!(
            lockfile.project.catalogs["legacy"][&Ident::new("bar")],
            zpm_primitives::Range::from_file_string("npm:^1.0.0").unwrap(),
        );

        let extension
            = lockfile.project.package_extensions.values().next().unwrap();

        assert_eq!(extension.dependencies.keys().cloned().collect::<Vec<_>>(), vec![Ident::new("bar")]);
        assert_eq!(extension.peer_dependencies_meta[&Ident::new("baz")].optional, Some(true));
    }

    #[test]
    fn should_discard_the_workspaces_stored_by_older_versions() {
        let lockfile: Lockfile = JsonDocument::hydrate_from_str(r#"{
            "__metadata": {"version": 9},
            "workspaces": {
                "root": "786a02f742015903c6c6fd852552d272912f4740e15847618a86e217f71f5419d25e1031afee585313896444934eb04b903a685b1448b755d56f701afe9be2ce"
            },
            "entries": {
                "lib@npm:^1.0.0": {
                    "checksum": null,
                    "resolution": {"resolution": "lib@workspace:lib", "version": "1.0.0"}
                }
            }
        }"#).unwrap();

        // The hashes older versions stored at the top-level covered the whole
        // dependency tree, so they can't be compared with the current ones.
        assert!(lockfile.project.workspaces.is_empty());

        assert!(lockfile.resolutions.is_empty());
        assert!(lockfile.entries.is_empty());
    }

    #[test]
    fn should_tolerate_project_sections_it_cannot_understand() {
        let lockfile: Lockfile = JsonDocument::hydrate_from_str(r#"{
            "__metadata": {"version": 9},
            "project": {
                "workspaces": {"root": "786a02f742015903"},
                "dependencyOverrides": {"this is not/a valid@selector/at all": 42}
            },
            "entries": {
                "foo@npm:^1.0.0": {
                    "checksum": null,
                    "resolution": {"resolution": "foo@npm:1.0.0", "version": "1.0.0"}
                }
            }
        }"#).unwrap();

        assert_eq!(lockfile.project, Default::default());
        assert_eq!(lockfile.resolutions.len(), 1);
    }
}
