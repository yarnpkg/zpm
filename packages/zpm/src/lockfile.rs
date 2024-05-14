use std::{collections::{BTreeMap, HashMap}, fmt, marker::PhantomData, sync::Arc};

use itertools::Itertools;
use serde::{de::{self, Visitor}, Deserialize, Deserializer, Serialize, Serializer};
use zpm_macros::track_time;

use crate::{error::Error, hash::Sha256, primitives::{Descriptor, Locator}, resolver::Resolution, serialize::Serialized};

#[derive(Clone, Debug, Eq, PartialEq, Hash, PartialOrd, Ord)]
struct MultiKey<T>(Vec<T>);

impl<T> MultiKey<T> {
    fn new() -> Self {
        MultiKey(vec![])
    }
}

impl<T> Serialize for MultiKey<T> where T: Serialized {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where S: Serializer {
        let mut string = String::new();

        for (index, item) in self.0.iter().enumerate() {
            if index > 0 {
                string.push_str(", ");
            }

            string.push_str(&item.serialized().unwrap());
        }

        serializer.serialize_str(&string)
    }
}

impl<'de, T> Deserialize<'de> for MultiKey<T> where T: std::str::FromStr {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> where D: Deserializer<'de> {
        struct VecVisitor<T> {
            marker: PhantomData<fn() -> T>,
        }

        impl<'de, T> Visitor<'de> for VecVisitor<T> where T: std::str::FromStr {
            type Value = Vec<T>;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a string of comma-separated values")
            }

            fn visit_str<E>(self, value: &str) -> Result<Vec<T>, E> where E: de::Error {
                value
                    .split(',')
                    .map(str::trim)
                    .map(|s| T::from_str(s).map_err(|_| de::Error::custom("TODO: forward errors")))
                    .collect()
            }
        }

        let visitor = VecVisitor { marker: PhantomData };
        deserializer.deserialize_str(visitor).map(MultiKey)
    }
}

#[derive(Deserialize, Serialize)]
struct LockfileMetadata {
    version: u64,
    cache_key: u64,
    linker_key: u64,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct LockfileEntry {
    pub checksum: Option<Sha256>,

    #[serde(flatten)]
    pub resolution: Resolution,
}

#[derive(Deserialize, Serialize)]
struct LockfilePayload {
    #[serde(rename = "__metadata")]
    metadata: LockfileMetadata,

    #[serde(flatten)]
    entries: BTreeMap<MultiKey<Descriptor>, LockfileEntry>,
}

#[derive(Clone, Default)]
pub struct Lockfile {
    pub resolutions: HashMap<Descriptor, Locator>,
    pub entries: HashMap<Locator, LockfileEntry>,
}

impl Lockfile {
    pub fn new() -> Self {
        Self {
            resolutions: HashMap::new(),
            entries: HashMap::new(),
        }
    }
}

struct MultiKeyLockfileEntry {
    key: MultiKey<Descriptor>,
    inner: LockfileEntry,
}

impl MultiKeyLockfileEntry {
    fn new(inner: LockfileEntry) -> Self {
        Self {
            key: MultiKey::new(),
            inner,
        }
    }
}

pub fn deserialize(src: &str) -> Result<Lockfile, Error> {
    let data = serde_json::from_str::<LockfilePayload>(src)
        .map_err(|err| Error::InvalidJsonData(Arc::new(err)))?;

    let mut lockfile = Lockfile {
        resolutions: HashMap::new(),
        entries: HashMap::new(),
    };

    for (key, entry) in data.entries {
        for descriptor in key.0 {
            lockfile.resolutions.insert(descriptor, entry.resolution.locator.clone());
        }

        lockfile.entries.insert(entry.resolution.locator.clone(), entry);
    }

    Ok(lockfile)
}

#[track_time]
pub fn serialize(entries: HashMap<Descriptor, LockfileEntry>) -> Result<String, Error> {
    let mut descriptors_to_resolutions: HashMap<Locator, MultiKeyLockfileEntry> = HashMap::new();
    for (descriptor, entry) in entries.into_iter().sorted_by_key(|(descriptor, _)| descriptor.clone()) {
        descriptors_to_resolutions.entry(entry.resolution.locator.clone())
            .or_insert_with(|| MultiKeyLockfileEntry::new(entry))
            .key.0
            .push(descriptor);
    }

    let mut entries = BTreeMap::new();
    for (_, entry) in descriptors_to_resolutions {
        entries.insert(entry.key, entry.inner);
    }

    let payload = LockfilePayload {
        metadata: LockfileMetadata {
            version: 0,
            cache_key: 0,
            linker_key: 0,
        },
        entries,
    };

    let output
        = serde_json::to_string_pretty(&payload).unwrap();

    Ok(output)
}
