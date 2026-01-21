use std::{collections::BTreeMap, fmt::{self, Debug, Display}, hash::Hash, marker::PhantomData, sync::Arc};

use rkyv::Archive;
use itertools::Itertools;
use serde::{de::{self, Visitor}, Deserialize, Deserializer, Serialize, Serializer};
use zpm_primitives::{Descriptor, Ident, Locator, Range, Reference, RegistryReference, RegistrySemverRange};
use zpm_utils::{FromFileString, Hash64, Path, ToFileString};

use crate::{
    error::Error,
    primitives_exts::RangeExt,
    resolvers::Resolution,
};

const LOCKFILE_VERSION: u64 = 9;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Archive, rkyv::Serialize, rkyv::Deserialize)]
#[rkyv(serialize_bounds(__S: rkyv::ser::Writer + rkyv::ser::Allocator + rkyv::ser::Sharing, <__S as rkyv::rancor::Fallible>::Error: rkyv::rancor::Source))]
#[rkyv(deserialize_bounds(__D: rkyv::de::Pooling, <__D as rkyv::rancor::Fallible>::Error: rkyv::rancor::Source))]
#[rkyv(bytecheck(bounds(__C: rkyv::validation::ArchiveContext + rkyv::validation::SharedContext, <__C as rkyv::rancor::Fallible>::Error: rkyv::rancor::Source)))]
pub struct LockfileEntry {
    pub checksum: Option<Hash64>,
    pub resolution: Resolution,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Archive, rkyv::Serialize, rkyv::Deserialize)]
#[rkyv(serialize_bounds(__S: rkyv::ser::Writer + rkyv::ser::Allocator + rkyv::ser::Sharing, <__S as rkyv::rancor::Fallible>::Error: rkyv::rancor::Source))]
#[rkyv(deserialize_bounds(__D: rkyv::de::Pooling, <__D as rkyv::rancor::Fallible>::Error: rkyv::rancor::Source))]
#[rkyv(bytecheck(bounds(__C: rkyv::validation::ArchiveContext + rkyv::validation::SharedContext, <__C as rkyv::rancor::Fallible>::Error: rkyv::rancor::Source)))]
pub struct Lockfile {
    pub metadata: LockfileMetadata,
    pub resolutions: BTreeMap<Descriptor, Locator>,
    pub entries: BTreeMap<Locator, LockfileEntry>,
}

impl Lockfile {
    pub fn new() -> Self {
        Self {
            metadata: LockfileMetadata::new(),
            resolutions: BTreeMap::new(),
            entries: BTreeMap::new(),
        }
    }
}

impl<'de> Deserialize<'de> for Lockfile {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> where D: Deserializer<'de> {
        let payload = LockfilePayload::deserialize(deserializer)?;

        let mut lockfile = Lockfile::new();

        lockfile.metadata = payload.metadata;

        for (key, entry) in payload.entries {
            for descriptor in key.0 {
                lockfile.resolutions.insert(descriptor, entry.resolution.locator.clone());
            }

            lockfile.entries.insert(entry.resolution.locator.clone(), entry);
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

        let mut descriptors_to_resolutions: BTreeMap<Locator, MultiKeyLockfileEntry> = BTreeMap::new();
        for (descriptor, locator) in self.resolutions.iter().sorted_by_key(|(descriptor, _)| (*descriptor).clone()) {
            // Skip descriptors with transient_resolution set to true
            if descriptor.range.details().transient_resolution {
                continue;
            }

            let entry = self.entries.get(locator)
                .expect("Expected a matching resolution to be found in the lockfile for any resolved locator.");

            descriptors_to_resolutions.entry(entry.resolution.locator.clone())
                .or_insert_with(|| MultiKeyLockfileEntry {inner: entry.clone(), key: MultiKey::new()})
                .key.0
                .push(descriptor.clone());
        }

        let mut entries = BTreeMap::new();
        for entry in descriptors_to_resolutions.into_values() {
            entries.insert(entry.key, entry.inner);
        }

        let payload = LockfilePayload {
            metadata: self.metadata.clone(),
            entries,
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

            string.push_str(&item.to_file_string());
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
                let result = value
                    .split(',')
                    .map(str::trim)
                    .map(|s| T::from_file_string(s))
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

#[derive(Deserialize, Serialize)]
struct LockfilePayload {
    #[serde(rename = "__metadata")]
    #[serde(default)]
    metadata: LockfileMetadata,

    #[serde(default)]
    entries: BTreeMap<MultiKey<Descriptor>, LockfileEntry>,
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

/// Minimal manifest structure for reading pnpm package.json files
#[derive(Debug, Deserialize)]
struct PnpmManifest {
    #[serde(default)]
    version: Option<String>,

    #[serde(default)]
    dependencies: Option<BTreeMap<String, String>>,
}

/// Scans `node_modules/.pnpm` to build a best-effort lockfile from pnpm's installed packages.
///
/// The approach:
/// 1. Read all package folders in `node_modules/.pnpm`
/// 2. For each package, read its package.json to get dependencies (as descriptors)
/// 3. Build a reverse map: descriptor -> path (where the descriptor was found)
/// 4. For each descriptor, read `../node_modules/<dep>/package.json` to find the resolved version
pub fn from_pnpm_node_modules(project_cwd: &Path) -> Result<Lockfile, Error> {
    let pnpm_dir
        = project_cwd.with_join_str("node_modules/.pnpm");

    if !pnpm_dir.fs_exists() {
        return Ok(Lockfile::new());
    }

    // Step 1: Scan all package folders in .pnpm and collect their dependencies
    // Maps descriptor -> list of paths where this descriptor appears
    let mut descriptor_to_paths: BTreeMap<_, Vec<_>>
        = BTreeMap::new();

    let entries
        = pnpm_dir.fs_read_dir()
            .map_err(|_| Error::PnpmNodeModulesReadError)?;

    for entry in entries {
        let Ok(entry) = entry else {
            continue;
        };

        let Ok(entry_path) = Path::try_from(entry.path()) else {
            continue;
        };

        // Skip non-directories and special entries
        if !entry_path.fs_is_dir() {
            continue;
        }

        let folder_name
            = entry_path.basename()
                .map(|s| s.to_string());

        let Some(folder_name) = folder_name else {
            continue;
        };

        // Skip special folders like .modules.yaml, lock files, etc.
        if folder_name.starts_with('.') {
            continue;
        }

        // Parse the folder name to get the package name
        // Format: <name>@<version> or @scope+name@version
        let package_name
            = parse_pnpm_folder_name(&folder_name);

        let Some(package_name) = package_name else {
            continue;
        };

        // The actual package.json is at:
        // node_modules/.pnpm/<folder>/node_modules/<package_name>/package.json
        let manifest_path = entry_path
            .with_join_str("node_modules")
            .with_join_str(&package_name)
            .with_join_str("package.json");

        if !manifest_path.fs_exists() {
            continue;
        }

        let manifest_content
            = manifest_path.fs_read_text();

        let Ok(manifest_content) = manifest_content else {
            continue;
        };

        let manifest: Result<PnpmManifest, _>
            = serde_json::from_str(&manifest_content);

        let Ok(manifest) = manifest else {
            continue;
        };

        // Collect dependencies as descriptors
        if let Some(dependencies) = manifest.dependencies {
            let package_node_modules = entry_path
                .with_join_str("node_modules");

            for (dep_name, dep_range) in dependencies {
                // Try to parse as semver range, skip if not recognized
                let Ok(range) = zpm_semver::Range::from_file_string(&dep_range) else {
                    continue;
                };

                let ident
                    = Ident::new(dep_name);

                let descriptor = Descriptor::new(ident, Range::RegistrySemver(RegistrySemverRange {
                    ident: None,
                    range,
                }));

                descriptor_to_paths.entry(descriptor)
                    .or_default()
                    .push(package_node_modules.clone());
            }
        }
    }

    // Step 2: For each descriptor, resolve it by reading the target package.json
    let mut lockfile
        = Lockfile::new();

    lockfile.metadata.version = 1;

    for (descriptor, paths) in descriptor_to_paths {
        // Use the first path that has the dependency
        let Some(base_path) = paths.first() else {
            continue;
        };

        // Build the path to the resolved package
        // For scoped packages, the ident contains the scope
        let dep_manifest_path = base_path
            .with_join_str(&descriptor.ident.to_file_string())
            .with_join_str("package.json");

        // Canonicalize to resolve symlinks
        let dep_manifest_path = dep_manifest_path.fs_canonicalize()
            .unwrap_or(dep_manifest_path);

        if !dep_manifest_path.fs_exists() {
            continue;
        }

        let dep_manifest_content
            = dep_manifest_path.fs_read_text();

        let Ok(dep_manifest_content) = dep_manifest_content else {
            continue;
        };

        let dep_manifest: Result<PnpmManifest, _>
            = serde_json::from_str(&dep_manifest_content);

        let Ok(dep_manifest) = dep_manifest else {
            continue;
        };

        let Some(version_str) = dep_manifest.version else {
            continue;
        };

        let Ok(version) = zpm_semver::Version::from_file_string(&version_str) else {
            continue;
        };

        let locator = Locator::new(descriptor.ident.clone(), RegistryReference {
            ident: descriptor.ident.clone(),
            version: version.clone(),
        }.into());

        lockfile.entries.insert(locator.clone(), LockfileEntry {
            checksum: None,
            resolution: Resolution::new_empty(locator.clone(), Default::default()),
        });

        lockfile.resolutions.insert(descriptor, locator);
    }

    Ok(lockfile)
}

/// Parses a pnpm folder name to extract the package name.
/// Examples:
/// - "lodash@4.17.21" -> "lodash"
/// - "@babel+core@7.0.0" -> "@babel/core"
/// - "@types+node@18.0.0_peer@1.0.0" -> "@types/node"
fn parse_pnpm_folder_name(folder_name: &str) -> Option<String> {
    // Handle scoped packages: @scope+name@version
    if folder_name.starts_with('@') {
        // Find the @ that separates name from version (not the scope @)
        let name_end = folder_name[1..].find('@')?;
        let name_part = &folder_name[..name_end + 1];
        // Convert + back to /
        Some(name_part.replace('+', "/"))
    } else {
        // Regular package: name@version
        let name_end = folder_name.find('@')?;
        Some(folder_name[..name_end].to_string())
    }
}
