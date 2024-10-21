use std::{collections::{HashMap, HashSet}, fmt, marker::PhantomData, str::FromStr, sync::{Arc, LazyLock}};

use arca::Path;
use bincode::{Decode, Encode};
use regex::Regex;
use serde::{de::{self, DeserializeSeed, IgnoredAny, Visitor}, Deserialize, Deserializer, Serialize};

use crate::{error::Error, fetcher::{fetch_folder_with_manifest, fetch_local_tarball_with_manifest, fetch_patched, fetch_remote_tarball_with_manifest, fetch_repository_with_manifest, PackageData}, formats::zip::ZipSupport, git::{resolve_git_treeish, GitRange, GitReference}, http::http_client, install::{InstallContext, InstallOpResult, ResolutionResult}, manifest::{parse_manifest, RemoteManifest}, primitives::{descriptor::{descriptor_map_deserializer, descriptor_map_serializer}, Descriptor, Ident, Locator, PeerRange, Range, Reference}, semver, serialize::UrlEncoded, system};

static NODE_GYP_IDENT: LazyLock<Ident> = LazyLock::new(|| Ident::from_str("node-gyp").unwrap());
static NODE_GYP_MATCH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b(node-gyp|prebuild-install)\b").unwrap());

/**
 * Contains the information we keep in the lockfile for a given package.
 */
#[derive(Clone, Debug, Deserialize, Decode, Encode, Serialize, PartialEq, Eq)]
pub struct Resolution {
    #[serde(rename = "resolution")]
    pub locator: Locator,
    pub version: semver::Version,

    #[serde(flatten)]
    pub requirements: system::Requirements,

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
            requirements: manifest.requirements,
        }
    }
}

pub async fn resolve(context: InstallContext<'_>, descriptor: Descriptor, dependencies: Vec<InstallOpResult>) -> Result<ResolutionResult, Error> {
    let mut resolution_result = resolve_direct(context, descriptor, dependencies).await?;

    normalize_resolution(&mut resolution_result.resolution);

    Ok(resolution_result)
}

pub fn normalize_resolution(resolution: &mut Resolution) {
    // Some protocols need to know about the package that declares the
    // dependency (for example the `portal:` protocol, which always points
    // to a location relative to the parent package. We mutate the
    // descriptors for these protocols to "bind" them to a particular
    // parent descriptor. In effect, it means we're creating a unique
    // version of the package, which will be resolved / fetched
    // independently from any other.
    //
    for descriptor in resolution.dependencies.values_mut() {
        if descriptor.range.must_bind() {
            descriptor.parent = Some(resolution.locator.clone());
        }
    }

    for name in resolution.peer_dependencies.keys().cloned().collect::<Vec<_>>() {
        resolution.peer_dependencies.entry(name.type_ident())
            .or_insert(PeerRange::Semver(semver::Range::from_str("*").unwrap()));
    }
}

async fn resolve_direct(context: InstallContext<'_>, descriptor: Descriptor, dependencies: Vec<InstallOpResult>) -> Result<ResolutionResult, Error> {
    match &descriptor.range {
        Range::SemverOrWorkspace(range)
            => resolve_semver_or_workspace(context, &descriptor.ident, range).await,

        Range::Git(range)
            => resolve_git(context, descriptor.ident, range).await,

        Range::Semver(range)
            => resolve_semver(context, &descriptor.ident, range).await,

        Range::SemverAlias(ident, range)
            => resolve_semver(context, ident, range).await,

        Range::Link(path)
            => resolve_link(&descriptor.ident, path, &descriptor.parent),

        Range::Url(url)
            => resolve_url(context, descriptor.ident, url).await,

        Range::Patch(_, file)
            => resolve_patch(context, descriptor.ident, file, &descriptor.parent, dependencies).await,

        Range::Tarball(path)
            => resolve_tarball(context, descriptor.ident, path, &descriptor.parent, dependencies).await,

        Range::Folder(path)
            => resolve_folder(context, descriptor.ident, path, &descriptor.parent, dependencies).await,

        Range::Portal(path)
            => resolve_portal(&descriptor.ident, path, &descriptor.parent, dependencies),

        Range::SemverTag(tag)
            => resolve_semver_tag(context, descriptor.ident, tag).await,

        Range::WorkspaceMagic(_)
            => resolve_workspace_by_name(context, descriptor.ident),

        Range::WorkspaceSemver(_)
            => resolve_workspace_by_name(context, descriptor.ident),

        Range::WorkspacePath(path)
            => resolve_workspace_by_path(context, path),

        _ => Err(Error::Unsupported),
    }
}

pub fn resolve_link(ident: &Ident, path: &str, parent: &Option<Locator>) -> Result<ResolutionResult, Error> {
    let resolution = Resolution {
        version: semver::Version::new(),
        locator: Locator::new_bound(ident.clone(), Reference::Link(path.to_string()), parent.clone().map(Arc::new)),
        dependencies: HashMap::new(),
        peer_dependencies: HashMap::new(),
        optional_dependencies: HashSet::new(),
        missing_peer_dependencies: HashSet::new(),
        requirements: system::Requirements::default(),
    };

    Ok(ResolutionResult::new(resolution))
}

pub async fn resolve_url(context: InstallContext<'_>, ident: Ident, url: &str) -> Result<ResolutionResult, Error> {
    let locator = Locator::new(ident.clone(), Reference::Url(url.to_string()));

    let fetch_result
        = fetch_remote_tarball_with_manifest(context, &locator, url).await?;

    Ok(fetch_result.into_resolution_result())
}

pub async fn resolve_patch(context: InstallContext<'_>, ident: Ident, path: &str, parent: &Option<Locator>, mut dependencies: Vec<InstallOpResult>) -> Result<ResolutionResult, Error> {
    let inner_locator
        = dependencies[1].as_resolved().resolution.locator.clone();

    let locator
        = Locator::new_bound(ident, Reference::Patch(Box::new(UrlEncoded::new(inner_locator)), path.to_string()), parent.clone().map(Arc::new));

    // We need to remove the "resolve" operation where we resolved the
    // descriptor into a locator before passing it to fetch
    dependencies.remove(1);

    let fetch_result
        = fetch_patched(context, &locator, path, dependencies).await?;

    Ok(fetch_result.into_resolution_result())
}

pub async fn resolve_tarball(context: InstallContext<'_>, ident: Ident, path: &str, parent: &Option<Locator>, dependencies: Vec<InstallOpResult>) -> Result<ResolutionResult, Error> {
    let locator = Locator::new_bound(ident, Reference::Tarball(path.to_string()), parent.clone().map(Arc::new));

    let fetch_result
        = fetch_local_tarball_with_manifest(context, &locator, path, dependencies).await?;

    Ok(fetch_result.into_resolution_result())
}

pub async fn resolve_folder(context: InstallContext<'_>, ident: Ident, path: &str, parent: &Option<Locator>, dependencies: Vec<InstallOpResult>) -> Result<ResolutionResult, Error> {
    let locator = Locator::new_bound(ident, Reference::Folder(path.to_string()), parent.clone().map(Arc::new));

    let fetch_result
        = fetch_folder_with_manifest(context, &locator, path, dependencies).await?;

    Ok(fetch_result.into_resolution_result())
}

pub fn resolve_portal(ident: &Ident, path: &str, parent: &Option<Locator>, dependencies: Vec<InstallOpResult>) -> Result<ResolutionResult, Error> {
    let parent_data = dependencies[0].as_fetched();

    let parent = parent.as_ref()
        .expect("The parent locator is required for resolving a portal package");

    let package_directory = parent_data.package_data
        .context_directory()
        .with_join_str(path);

    let manifest_path = package_directory
        .with_join_str("package.json");
    let manifest_text = manifest_path
        .fs_read_text_with_zip()?;
    let manifest
        = parse_manifest(manifest_text)?;

    let locator = Locator::new_bound(ident.clone(), Reference::Portal(path.to_string()), Some(Arc::new(parent.clone())));
    let resolution = Resolution::from_remote_manifest(locator, manifest.remote);

    Ok(ResolutionResult::new(resolution))
}

pub async fn resolve_git(context: InstallContext<'_>, ident: Ident, source: &GitRange) -> Result<ResolutionResult, Error> {
    let commit = resolve_git_treeish(source).await?;

    let git_reference = GitReference {
        repo: source.repo.clone(),
        commit: commit.clone(),
        prepare_params: source.prepare_params.clone(),
    };

    let locator
        = Locator::new(ident, Reference::Git(git_reference.clone()));

    let fetch_result
        = fetch_repository_with_manifest(context, &locator, &git_reference).await?;

    Ok(fetch_result.into_resolution_result())
}

pub async fn resolve_semver_tag(context: InstallContext<'_>, ident: Ident, tag: &str) -> Result<ResolutionResult, Error> {
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

    let manifest = registry_data.versions.remove(version).unwrap();

    let dist_manifest = manifest.dist
        .as_ref()
        .expect("Expected the registry to return a 'dist' field amongst the manifest data");

    let expected_registry_url
        = project.config.registry_url_for_package_data(&ident, &version);

    let reference = match expected_registry_url == dist_manifest.tarball {
        true => Reference::SemverAlias(ident.clone(), version.clone()),
        false => Reference::Url(dist_manifest.tarball.clone()),
    };

    let locator = Locator::new(ident.clone(), reference);
    let resolution = Resolution::from_remote_manifest(locator, manifest);

    Ok(ResolutionResult::new(resolution))
}

pub async fn resolve_semver(context: InstallContext<'_>, ident: &Ident, range: &semver::Range) -> Result<ResolutionResult, Error> {
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
                    selected = Some((version, map.next_value::<serde_json::Value>()?));
                } else {
                    map.next_value::<IgnoredAny>()?;
                }
            }
    
            Ok(selected.map(|(version, version_payload)| {
                (version, T::deserialize(version_payload).unwrap())
            }).unwrap())
        }
    }

    let project = context.project
        .expect("The project is required for resolving a workspace package");

    let client = http_client()?;

    let registry_url = project.config.registry_url_for(ident);
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

    #[derive(Clone, Deserialize)]
    struct RemoteManifestWithScripts {
        #[serde(flatten)]
        remote: RemoteManifest,

        #[serde(default)]
        #[serde(skip_serializing_if = "HashMap::is_empty")]
        scripts: HashMap<String, String>,
    }

    let manifest_result = deserializer.deserialize_map(FindField {
        field: "versions",
        nested: FindVersion {
            range: range.clone(),
            phantom: PhantomData::<RemoteManifestWithScripts>,
        },
    });

    let (version, mut manifest) = manifest_result
        .map_err(|_| Error::NoCandidatesFound(Range::Semver(range.clone())))?;

    // Manually add node-gyp dependency if there is a script using it and not already set
    // This is because the npm registry will automatically add a `node-gyp rebuild` install script
    // in the metadata if there is not already an install script and a binding.gyp file exists.
    // Also, node-gyp is not always set as a dependency in packages, so it will also be added if used in scripts.
    //
    if !manifest.remote.dependencies.contains_key(&NODE_GYP_IDENT) && !manifest.remote.peer_dependencies.contains_key(&NODE_GYP_IDENT) {
        for script in manifest.scripts.values() {
            if NODE_GYP_MATCH.is_match(script.as_str()) {
                manifest.remote.dependencies.insert(NODE_GYP_IDENT.clone(), Descriptor::new_semver(NODE_GYP_IDENT.clone(), "*").unwrap());
                break;
            }
        }
    }

    let dist_manifest = manifest.remote.dist
        .as_ref()
        .expect("Expected the registry to return a 'dist' field amongst the manifest data");

    let expected_registry_url
        = project.config.registry_url_for_package_data(ident, &version);

    let reference = match expected_registry_url == dist_manifest.tarball {
        true => Reference::Semver(version),
        false => Reference::Url(dist_manifest.tarball.clone()),
    };

    let locator = Locator::new(ident.clone(), reference);
    let resolution = Resolution::from_remote_manifest(locator, manifest.remote);

    Ok(ResolutionResult::new(resolution))
}

pub async fn resolve_semver_or_workspace(context: InstallContext<'_>, ident: &Ident, range: &semver::Range) -> Result<ResolutionResult, Error> {
    let project = context.project
        .expect("The project is required for resolving a workspace package");

    if project.config.project.enable_transparent_workspaces.value {
        if let Ok(workspace) = resolve_workspace_by_name(context.clone(), ident.clone()) {
            if range.check(&workspace.resolution.version) {
                return Ok(workspace);
            }
        }
    }

    resolve_semver(context, ident, range).await
}

pub fn resolve_workspace_by_name(context: InstallContext, ident: Ident) -> Result<ResolutionResult, Error> {
    let project = context.project
        .expect("The project is required for resolving a workspace package");

    match project.workspaces.get(&ident) {
        Some(workspace) => {
            let manifest = workspace.manifest.clone();

            let locator = Locator::new(ident.clone(), Reference::Workspace(workspace.name.clone()));
            let mut resolution = Resolution::from_remote_manifest(locator, manifest.remote);

            resolution.dependencies.extend(manifest.dev_dependencies);

            Ok(ResolutionResult::new(resolution))
        }

        None => Err(Error::WorkspaceNotFound(ident)),
    }
}

pub fn resolve_workspace_by_path(context: InstallContext, path: &str) -> Result<ResolutionResult, Error> {
    let project = context.project
        .expect("The project is required for resolving a workspace package");

    if let Some(ident) = project.workspaces_by_rel_path.get(&Path::from(path)) {
        resolve_workspace_by_name(context, ident.clone())
    } else {
        Err(Error::WorkspacePathNotFound())
    }
}
