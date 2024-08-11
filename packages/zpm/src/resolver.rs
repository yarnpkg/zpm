use std::{collections::{HashMap, HashSet}, fmt, marker::PhantomData, str::FromStr, sync::Arc};

use bincode::{Decode, Encode};
use serde::{de::{self, DeserializeSeed, IgnoredAny, Visitor}, Deserialize, Deserializer, Serialize};

use crate::{error::Error, fetcher::{fetch_folder_with_manifest, fetch_local_tarball_with_manifest, fetch_remote_tarball_with_manifest, PackageData}, git::{resolve_git_treeish, GitRange}, http::http_client, install::InstallContext, manifest::{parse_manifest, RemoteManifest}, primitives::{descriptor::{descriptor_map_deserializer, descriptor_map_serializer}, Descriptor, Ident, Locator, PeerRange, Range, Reference}, semver};

pub struct ResolveResult {
    pub resolution: Resolution,
    pub package_data: Option<PackageData>,
}

impl ResolveResult {
    pub fn new(resolution: Resolution) -> Self {
        ResolveResult {
            resolution,
            package_data: None,
        }
    }

    pub fn new_with_data(resolution: Resolution, package_data: PackageData) -> Self {
        ResolveResult {
            resolution,
            package_data: Some(package_data),
        }
    }
}

/**
 * Contains the information we keep in the lockfile for a given package.
 */
#[derive(Clone, Debug, Deserialize, Decode, Encode, Serialize, PartialEq, Eq)]
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

    #[serde(default)]
    #[serde(skip_serializing_if = "HashSet::is_empty")]
    pub missing_peer_dependencies: HashSet<Ident>,
}

impl Resolution {
    pub fn from_remote_manifest(locator: Locator, manifest: RemoteManifest) -> Resolution {
        let optional_dependencies
            = HashSet::from_iter(manifest.optional_dependencies.keys().cloned());

        let mut dependencies
            = manifest.dependencies;

        dependencies
            .extend(manifest.optional_dependencies);

        Resolution {
            locator,
            version: manifest.version,
            dependencies,
            peer_dependencies: manifest.peer_dependencies,
            optional_dependencies,
            missing_peer_dependencies: HashSet::new(),
        }
    }
}

pub async fn resolve<'a>(context: InstallContext<'a>, descriptor: Descriptor, parent_data: Option<PackageData>) -> Result<ResolveResult, Error> {
    match &descriptor.range {
        Range::SemverOrWorkspace(range)
            => resolve_semver_or_workspace(context, &descriptor.ident, range).await,

        Range::Git(range)
            => resolve_git(descriptor.ident, range).await,

        Range::Semver(range)
            => resolve_semver(context, &descriptor.ident, range).await,

        Range::SemverAlias(ident, range)
            => resolve_semver(context, ident, range).await,

        Range::Link(path)
            => resolve_link(&descriptor.ident, path, &descriptor.parent),

        Range::Url(url)
            => resolve_url(context, descriptor.ident, url).await,

        Range::Tarball(path)
            => resolve_tarball(context, descriptor.ident, path, &descriptor.parent, parent_data).await,

        Range::Folder(path)
            => resolve_folder(context, descriptor.ident, path, &descriptor.parent, parent_data).await,

        Range::Portal(path)
            => resolve_portal(&descriptor.ident, path, &descriptor.parent, parent_data),

        Range::SemverTag(tag)
            => resolve_semver_tag(context, descriptor.ident, tag).await,

        Range::WorkspaceMagic(_)
            => resolve_workspace_by_name(context, descriptor.ident),

        Range::WorkspaceSemver(_)
            => resolve_workspace_by_name(context, descriptor.ident),

        _ => Err(Error::Unsupported),
    }
}

pub fn resolve_link(ident: &Ident, path: &str, parent: &Option<Locator>) -> Result<ResolveResult, Error> {
    let resolution = Resolution {
        version: semver::Version::new(),
        locator: Locator::new_bound(ident.clone(), Reference::Link(path.to_string()), parent.clone().map(Arc::new)),
        dependencies: HashMap::new(),
        peer_dependencies: HashMap::new(),
        optional_dependencies: HashSet::new(),
        missing_peer_dependencies: HashSet::new(),
    };

    Ok(ResolveResult::new(resolution))
}

pub async fn resolve_url<'a>(context: InstallContext<'a>, ident: Ident, url: &str) -> Result<ResolveResult, Error> {
    let locator = Locator::new(ident.clone(), Reference::Url(url.to_string()));

    let (resolution, package_data)
        = fetch_remote_tarball_with_manifest(context, &locator, url).await?;

    Ok(ResolveResult::new_with_data(resolution, package_data))
}

pub async fn resolve_tarball<'a>(context: InstallContext<'a>, ident: Ident, path: &str, parent: &Option<Locator>, parent_data: Option<PackageData>) -> Result<ResolveResult, Error> {
    let locator = Locator::new_bound(ident, Reference::Tarball(path.to_string()), parent.clone().map(Arc::new));

    let (resolution, package_data)
        = fetch_local_tarball_with_manifest(context, &locator, path, parent_data).await?;

    Ok(ResolveResult::new_with_data(resolution, package_data))
}

pub async fn resolve_folder<'a>(context: InstallContext<'a>, ident: Ident, path: &str, parent: &Option<Locator>, parent_data: Option<PackageData>) -> Result<ResolveResult, Error> {
    let locator = Locator::new_bound(ident, Reference::Folder(path.to_string()), parent.clone().map(Arc::new));

    let (resolution, package_data)
        = fetch_folder_with_manifest(context, &locator, path, parent_data).await?;

    Ok(ResolveResult::new_with_data(resolution, package_data))
}

pub fn resolve_portal(ident: &Ident, path: &str, parent: &Option<Locator>, parent_data: Option<PackageData>) -> Result<ResolveResult, Error> {
    let parent = parent.as_ref()
        .expect("The parent locator is required for resolving a portal package");
    let parent_data = parent_data
        .expect("The parent data is required for retrieving the path of a portal package");

    let package_directory = parent_data.context_directory()
        .with_join_str(&path);

    let manifest_path = package_directory
        .with_join_str("package.json");
    let manifest_text
        = parent_data.read_text(&manifest_path)?;
    let manifest
        = parse_manifest(manifest_text)?;

    let locator = Locator::new_bound(ident.clone(), Reference::Portal(path.to_string()), Some(Arc::new(parent.clone())));
    let resolution = Resolution::from_remote_manifest(locator, manifest.remote);

    Ok(ResolveResult::new(resolution))
}

pub async fn resolve_git(ident: Ident, git_range: &GitRange) -> Result<ResolveResult, Error> {
    let commit = resolve_git_treeish(&git_range).await?;

    let locator = Locator::new(ident, Reference::Git(GitRange {
        repo: git_range.repo.clone(),
        treeish: crate::git::GitTreeish::Commit(commit),
    }));

    let resolution = Resolution {
        version: semver::Version::new(),
        locator,
        dependencies: HashMap::new(),
        peer_dependencies: HashMap::new(),
        optional_dependencies: HashSet::new(),
        missing_peer_dependencies: HashSet::new(),
    };

    Ok(ResolveResult::new(resolution))
}

pub async fn resolve_semver_tag<'a>(context: InstallContext<'a>, ident: Ident, tag: &str) -> Result<ResolveResult, Error> {
    let project = context.project
        .expect("The project is required for resolving a workspace package");

    let client = http_client()?;

    let registry_url = project.config.registry_url_for(&ident);
    let url = format!("{}/{}", registry_url, ident);

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

    let version = registry_data.dist_tags.get(tag)
        .ok_or_else(|| Error::MissingSemverTag(tag.to_string()))?;

    let manifest = registry_data.versions.remove(&version).unwrap();

    let locator = Locator::new(ident.clone(), Reference::SemverAlias(ident.clone(), version.clone()));
    let resolution = Resolution::from_remote_manifest(locator, manifest);

    Ok(ResolveResult::new(resolution))
}

pub async fn resolve_semver<'a>(context: InstallContext<'a>, ident: &Ident, range: &semver::Range) -> Result<ResolveResult, Error> {
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

    let project = context.project
        .expect("The project is required for resolving a workspace package");

    let client = http_client()?;

    let registry_url = project.config.registry_url_for(&ident);
    let url = format!("{}/{}", registry_url, ident);

    let response = client.get(url.clone()).send().await
        .map_err(|err| Error::RemoteRegistryError(Arc::new(err)))?;

    if response.status().as_u16() == 404 {
        return Err(Error::PackageNotFound(ident.clone(), url));
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
        .map_err(|_| Error::NoCandidatesFound(Range::Semver(range.clone())))?;

    let locator = Locator::new(ident.clone(), Reference::Semver(version));
    let resolution = Resolution::from_remote_manifest(locator, manifest);

    Ok(ResolveResult::new(resolution))
}

pub async fn resolve_semver_or_workspace<'a>(context: InstallContext<'a>, ident: &Ident, range: &semver::Range) -> Result<ResolveResult, Error> {
    if let Ok(workspace) = resolve_workspace_by_name(context.clone(), ident.clone()) {
        return Ok(workspace);
    }

    resolve_semver(context, ident, range).await
}

pub fn resolve_workspace_by_name(context: InstallContext, ident: Ident) -> Result<ResolveResult, Error> {
    let project = context.project
        .expect("The project is required for resolving a workspace package");

    match project.workspaces.get(&ident) {
        Some(workspace) => {
            let manifest = workspace.manifest.clone();

            let locator = Locator::new(ident.clone(), Reference::Workspace(workspace.name.clone()));
            let mut resolution = Resolution::from_remote_manifest(locator, manifest.remote);

            resolution.dependencies.extend(manifest.dev_dependencies);

            Ok(ResolveResult::new(resolution))
        }

        None => Err(Error::WorkspaceNotFound(ident)),
    }
}
