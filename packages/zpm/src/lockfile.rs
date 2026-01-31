use std::{collections::BTreeMap, fmt::{self, Debug, Display}, hash::Hash, marker::PhantomData, sync::Arc};

use rkyv::Archive;
use itertools::Itertools;
use serde::{de::{self, Visitor}, Deserialize, Deserializer, Serialize, Serializer};
use zpm_config::{Configuration, ConfigurationContext};
use zpm_parsers::JsonDocument;
use zpm_primitives::{Descriptor, Ident, Locator, Range, Reference, RegistryReference, RegistrySemverRange};
use zpm_utils::{FromFileString, Hash64, LastModifiedAt, Path, ToFileString, UrlEncoded};

use crate::{
    error::Error, http_npm, npm, primitives_exts::RangeExt, resolvers::Resolution
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

            let _ = item.write_file_string(&mut string);
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
/// 1. Run `pnpm list --json --depth=3` to get most of the full dependency tree
/// 2. Recursively walk the tree to collect all packages with their resolved URLs
/// 3. For each package, read its package.json to get the original dependency ranges
/// 4. Build descriptor -> locator mappings
pub fn from_pnpm_node_modules(project_cwd: &Path) -> Result<Lockfile, Error> {
    let user_cwd
        = Path::home_dir()?;

    let configuration_context = ConfigurationContext {
        env: std::env::vars().collect(),
        user_cwd: user_cwd.clone(),
        project_cwd: Some(project_cwd.clone()),
        package_cwd: None,
    };

    let mut last_modified_at
        = LastModifiedAt::new();

    let config
        = Configuration::load(&configuration_context, &mut last_modified_at)
            .map_err(|e| Error::ConfigurationParseError(Arc::new(e)))?;

    let pnpm_dir
        = project_cwd
            .with_join_str("node_modules/.pnpm");

    if !pnpm_dir.fs_exists() {
        return Ok(Lockfile::new());
    }

    // Run pnpm list --json --depth=Infinity
    let output = std::process::Command::new("pnpm")
        .args(["list", "-r", "--json", "--depth=3"])
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

        let Ok(package_path) = Path::try_from(package_path_str.as_str()) else {
            continue;
        };

        #[derive(Debug, Deserialize)]
        struct Manifest {
            #[serde(default)]
            dependencies: BTreeMap<String, String>,
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

        for (name, range) in manifest.dependencies {
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
                = http_npm::get_registry(&config, ident.scope(), false)?;

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
