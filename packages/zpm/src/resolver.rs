use std::{collections::{HashMap, HashSet}, fmt, marker::PhantomData, str::FromStr, sync::Arc};

use bincode::{Decode, Encode};
use serde::{de::{self, DeserializeSeed, IgnoredAny, Visitor}, Deserialize, Deserializer, Serialize};

use crate::{config, error::Error, git::{resolve_git_treeish, GitRange}, http::http_client, manifest::RemoteManifest, primitives::{descriptor::{descriptor_map_deserializer, descriptor_map_serializer}, Descriptor, Ident, Locator, PeerRange, Range, Reference}, project, semver};

/**
 * Contains the information we keep in the lockfile for a given package.
 */
#[derive(Clone, Debug, Deserialize, Decode, Encode, Serialize)]
pub struct Resolution {
    #[serde(rename = "resolution")]
    pub locator: Locator,
    pub version: semver::Version,

    #[serde(default)]
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    #[serde(serialize_with = "descriptor_map_serializer")]
    #[serde(deserialize_with = "descriptor_map_deserializer")]
    pub dependencies: HashMap<Ident, Descriptor>,

    #[serde(default)]
    #[serde(rename = "peerDependencies")]
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub peer_dependencies: HashMap<Ident, PeerRange>,

    #[serde(default)]
    #[serde(rename = "optionalDependencies")]
    #[serde(skip_serializing_if = "HashSet::is_empty")]
    pub optional_dependencies: HashSet<Ident>,
}

pub async fn resolve(descriptor: Descriptor) -> Result<Resolution, Error> {
    match descriptor.range.clone() {
        Range::Git(range)
            => resolve_git(descriptor.ident, range).await,

        Range::Semver(range)
            => resolve_semver(descriptor.ident, range).await,

        Range::SemverAlias(ident, range)
            => resolve_semver(ident, range).await,

        Range::Link(path)
            => resolve_link(descriptor.ident, descriptor.parent, path),
    
        Range::SemverTag(tag)
            => resolve_semver_tag(descriptor.ident, tag).await,

        Range::WorkspaceMagic(_)
            => resolve_workspace_by_name(descriptor.ident),

        Range::WorkspaceSemver(_)
            => resolve_workspace_by_name(descriptor.ident),

        _ => Err(Error::Unsupported),
    }
}

pub fn resolve_link(ident: Ident, parent: Option<Locator>, path: String) -> Result<Resolution, Error> {
    Ok(Resolution {
        version: semver::Version::new(),
        locator: Locator::new_bound(ident.clone(), Reference::Link(path), parent.map(Arc::new)),
        dependencies: HashMap::new(),
        peer_dependencies: HashMap::new(),
        optional_dependencies: HashSet::new(),
    })
}

pub async fn resolve_git(ident: Ident, git_range: GitRange) -> Result<Resolution, Error> {
    let commit = resolve_git_treeish(&git_range).await?;

    let locator = Locator::new(ident, Reference::Git(GitRange {
        repo: git_range.repo,
        treeish: crate::git::GitTreeish::Commit(commit),
    }));

    Ok(Resolution {
        version: semver::Version::new(),
        locator,
        dependencies: HashMap::new(),
        peer_dependencies: HashMap::new(),
        optional_dependencies: HashSet::new(),
    })
}

pub async fn resolve_semver_tag(ident: Ident, tag: String) -> Result<Resolution, Error> {
    let client = http_client()?;
    let url = format!("{}/{}", config::registry_url_for(&ident), ident);

    let response = client.get(url.clone()).send().await
        .map_err(|err| Error::RemoteRegistryError(Arc::new(err)))?;

    let registry_text = response.text().await
        .map_err(|err| Error::RemoteRegistryError(Arc::new(err)))?;

    #[derive(Deserialize)]
    struct RegistryMetadata {
        #[serde(rename(deserialize = "dist-tags"))]
        dist_tags: HashMap<String, semver::Version>,
        versions: HashMap<semver::Version, RemoteManifest>,
    }

    let mut registry_data: RegistryMetadata = serde_json::from_str(registry_text.as_str())
        .map_err(Arc::new)?;

    let version = registry_data.dist_tags.get(tag.as_str())
        .ok_or(Error::MissingSemverTag(tag))?;

    let manifest = registry_data.versions.remove(&version).unwrap();

    Ok(Resolution {
        version: manifest.version,
        locator: Locator::new(ident.clone(), Reference::SemverAlias(ident.clone(), version.clone())),
        dependencies: manifest.dependencies.unwrap_or_default(),
        peer_dependencies: manifest.peer_dependencies.unwrap_or_default(),
        optional_dependencies: HashSet::new(),
    })
}

pub async fn resolve_semver(ident: Ident, range: semver::Range) -> Result<Resolution, Error> {
    pub struct FindField<'a, T> {
        field: &'a str,
        nested: T,
    }
    
    impl<'de, T> Visitor<'de> for FindField<'_, T> where T: DeserializeSeed<'de> + Clone {
        type Value = T::Value;
    
        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            write!(formatter, "a map with a matching field")
        }
    
        fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error> where A: de::MapAccess<'de> {
            let mut selected = None;
    
            while let Some(key) = map.next_key::<String>()? {
                if key == self.field {
                    selected = Some(map.next_value_seed(self.nested.clone())?);
                } else {
                    let _ = map.next_value::<IgnoredAny>();
                }
            }
    
            selected
                .ok_or(de::Error::missing_field(""))
        }
    }
    
    #[derive(Clone)]
    pub struct FindVersion<T> {
        range: semver::Range,
        phantom: PhantomData<T>,
    }
    
    impl<'de, T> DeserializeSeed<'de> for FindVersion<T> where T: Deserialize<'de> {
        type Value = (semver::Version, T);
    
        fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error> where D: Deserializer<'de> {
            deserializer.deserialize_map(self)
        }
    }
    
    impl<'de, T> Visitor<'de> for FindVersion<T> where T: Deserialize<'de> {
        type Value = (semver::Version, T);
    
        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            write!(formatter, "a map with a matching version")
        }
    
        fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error> where A: de::MapAccess<'de> {
            let mut selected = None;
    
            while let Some(key) = map.next_key::<String>()? {
                let version = semver::Version::from_str(key.as_str()).unwrap();
    
                if self.range.check(&version) && selected.as_ref().map(|(current_version, _)| *current_version < version).unwrap_or(true) {
                    selected = Some((version, map.next_value::<T>()?));
                } else {
                    map.next_value::<IgnoredAny>()?;
                }
            }
    
            Ok(selected.unwrap())
        }
    }

    let client = http_client()?;
    let url = format!("{}/{}", config::registry_url_for(&ident), ident);

    let response = client.get(url.clone()).send().await
        .map_err(|err| Error::RemoteRegistryError(Arc::new(err)))?;

    if response.status().as_u16() == 404 {
        return Err(Error::PackageNotFound(ident, url));
    }
 
    let registry_text = response.text().await
        .map_err(|err| Error::RemoteRegistryError(Arc::new(err)))?;

    let mut deserializer
        = serde_json::Deserializer::from_str(registry_text.as_str());

    let manifest_result = deserializer.deserialize_map(FindField {
        field: "versions",
        nested: FindVersion {
            range: range.clone(),
            phantom: PhantomData::<RemoteManifest>,
        },
    });

    let (version, manifest) = manifest_result
        .map_err(|_| Error::NoCandidatesFound(Range::Semver(range)))?;

    let transitive_dependencies = manifest.dependencies.clone()
        .unwrap_or_default();

    Ok(Resolution {
        version: manifest.version,
        locator: Locator::new(ident, Reference::Semver(version)),
        dependencies: transitive_dependencies,
        peer_dependencies: manifest.peer_dependencies.unwrap_or_default(),
        optional_dependencies: HashSet::new(),
    })
}

pub fn resolve_workspace_by_name(ident: Ident) -> Result<Resolution, Error> {
    let workspaces = project::workspaces()?;

    match workspaces.get(&ident) {
        Some(workspace) => {
            let mut dependencies = workspace.manifest.dependencies.clone()
                .unwrap_or_default();

            let dev_dependencies = workspace.manifest.dev_dependencies.clone()
                .unwrap_or_default();

            dependencies.extend(dev_dependencies);

            let peer_dependencies = workspace.manifest.peer_dependencies.clone()
                .unwrap_or_default();

            Ok(Resolution {
                version: workspace.manifest.version.clone(),
                locator: Locator::new(ident.clone(), Reference::Workspace(ident.clone())),
                dependencies,
                peer_dependencies,
                optional_dependencies: HashSet::new(),
            })
        }

        None => Err(Error::WorkspaceNotFound(ident)),
    }
}
