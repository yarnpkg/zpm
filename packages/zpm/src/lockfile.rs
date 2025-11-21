use std::{collections::BTreeMap, fmt::{self, Debug, Display}, hash::Hash, marker::PhantomData, sync::Arc};

use bincode::{Decode, Encode};
use itertools::Itertools;
use serde::{de::{self, Visitor}, Deserialize, Deserializer, Serialize, Serializer};
use zpm_primitives::{Descriptor, Locator};
use zpm_utils::{FromFileString, Hash64, ToFileString};

use crate::{
    error::Error,
    primitives_exts::RangeExt,
    resolvers::Resolution,
};

const LOCKFILE_VERSION: u64 = 9;

#[derive(Clone, Debug, Encode, Decode, Serialize, Deserialize, PartialEq, Eq)]
pub struct LockfileEntry {
    pub checksum: Option<Hash64>,
    pub resolution: Resolution,
}

#[derive(Clone, Debug, Default, Encode, Decode, PartialEq, Eq)]
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

#[derive(Clone, Debug, Encode, Decode, Deserialize, Serialize, PartialEq, Eq)]
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
        for descriptor in key.0 {
            lockfile.resolutions.insert(descriptor, entry.resolution.clone());
        }

        lockfile.entries.insert(entry.resolution.clone(), LockfileEntry {
            checksum: None,
            resolution: Resolution {
                locator: entry.resolution,
                version: Default::default(),
                requirements: Default::default(),
                dependencies: Default::default(),
                peer_dependencies: Default::default(),
                optional_dependencies: Default::default(),
                optional_peer_dependencies: Default::default(),
                missing_peer_dependencies: Default::default(),
            },
        });
    }

    Ok(lockfile)
}

#[cfg(test)]
mod tests {
    use super::*;
    use zpm_utils::FromFileString;

    #[test]
    fn test_transient_resolutions_are_omitted_from_serialization() {
        // Create a lockfile with both transient and non-transient descriptors
        let mut lockfile = Lockfile::new();
        
        // Create a non-transient descriptor (npm registry package - has transient_resolution = false)
        let non_transient_descriptor = Descriptor::from_file_string("package-a@npm:^1.0.0")
            .expect("Failed to parse non-transient descriptor");
        
        // Create a transient descriptor (file: - has transient_resolution = true)
        let transient_descriptor = Descriptor::from_file_string("package-b@file:./path/to/folder")
            .expect("Failed to parse transient descriptor");
        
        // Create locators for both
        let non_transient_locator = Locator::from_file_string("package-a@npm:1.0.0")
            .expect("Failed to parse non-transient locator");
        
        let transient_locator = Locator::from_file_string("package-b@file:./path/to/folder")
            .expect("Failed to parse transient locator");
        
        // Create resolution entries
        let non_transient_entry = LockfileEntry {
            checksum: None,
            resolution: Resolution {
                locator: non_transient_locator.clone(),
                version: Default::default(),
                requirements: Default::default(),
                dependencies: Default::default(),
                peer_dependencies: Default::default(),
                optional_dependencies: Default::default(),
                optional_peer_dependencies: Default::default(),
                missing_peer_dependencies: Default::default(),
            },
        };
        
        let transient_entry = LockfileEntry {
            checksum: None,
            resolution: Resolution {
                locator: transient_locator.clone(),
                version: Default::default(),
                requirements: Default::default(),
                dependencies: Default::default(),
                peer_dependencies: Default::default(),
                optional_dependencies: Default::default(),
                optional_peer_dependencies: Default::default(),
                missing_peer_dependencies: Default::default(),
            },
        };
        
        // Add both to the lockfile
        lockfile.resolutions.insert(non_transient_descriptor.clone(), non_transient_locator.clone());
        lockfile.resolutions.insert(transient_descriptor.clone(), transient_locator.clone());
        lockfile.entries.insert(non_transient_locator.clone(), non_transient_entry);
        lockfile.entries.insert(transient_locator.clone(), transient_entry);
        
        // Serialize the lockfile
        let serialized = serde_yaml::to_string(&lockfile).unwrap();
        
        // The serialized output should include the non-transient descriptor
        assert!(serialized.contains("package-a"), "Non-transient descriptor should be in serialized output");
        
        // The serialized output should NOT include the transient descriptor
        assert!(!serialized.contains("package-b"), "Transient descriptor should NOT be in serialized output");
    }

    #[test]
    fn test_workspace_and_link_ranges_are_transient() {
        // Create a lockfile with workspace and link descriptors
        let mut lockfile = Lockfile::new();
        
        // Create a workspace descriptor (has transient_resolution = true)
        let workspace_descriptor = Descriptor::from_file_string("my-pkg@workspace:*")
            .expect("Failed to parse workspace descriptor");
        
        // Create a link descriptor (has transient_resolution = true)
        let link_descriptor = Descriptor::from_file_string("linked-pkg@link:../linked")
            .expect("Failed to parse link descriptor");
        
        // Create locators for both
        let workspace_locator = Locator::from_file_string("my-pkg@workspace:packages/my-pkg")
            .expect("Failed to parse workspace locator");
        
        let link_locator = Locator::from_file_string("linked-pkg@link:../linked")
            .expect("Failed to parse link locator");
        
        // Create resolution entries
        let workspace_entry = LockfileEntry {
            checksum: None,
            resolution: Resolution {
                locator: workspace_locator.clone(),
                version: Default::default(),
                requirements: Default::default(),
                dependencies: Default::default(),
                peer_dependencies: Default::default(),
                optional_dependencies: Default::default(),
                optional_peer_dependencies: Default::default(),
                missing_peer_dependencies: Default::default(),
            },
        };
        
        let link_entry = LockfileEntry {
            checksum: None,
            resolution: Resolution {
                locator: link_locator.clone(),
                version: Default::default(),
                requirements: Default::default(),
                dependencies: Default::default(),
                peer_dependencies: Default::default(),
                optional_dependencies: Default::default(),
                optional_peer_dependencies: Default::default(),
                missing_peer_dependencies: Default::default(),
            },
        };
        
        // Add both to the lockfile
        lockfile.resolutions.insert(workspace_descriptor.clone(), workspace_locator.clone());
        lockfile.resolutions.insert(link_descriptor.clone(), link_locator.clone());
        lockfile.entries.insert(workspace_locator.clone(), workspace_entry);
        lockfile.entries.insert(link_locator.clone(), link_entry);
        
        // Serialize the lockfile
        let serialized = serde_yaml::to_string(&lockfile).unwrap();
        
        // Neither workspace nor link descriptors should be in the serialized output
        assert!(!serialized.contains("my-pkg"), "Workspace descriptor should NOT be in serialized output");
        assert!(!serialized.contains("linked-pkg"), "Link descriptor should NOT be in serialized output");
    }
}
