use std::collections::{HashMap, HashSet};

use bincode::{Decode, Encode};
use serde::{Deserialize, Serialize};

use crate::{error::Error, install::{normalize_resolutions, InstallContext, InstallOpResult, IntoResolutionResult, ResolutionResult}, manifest::RemoteManifest, primitives::{descriptor::{descriptor_map_deserializer, descriptor_map_serializer}, range, Descriptor, Ident, Locator, PeerRange, Range, Reference}, system};

mod folder;
mod git;
mod link;
mod patch;
mod portal;
mod npm;
mod semver;
mod tag;
mod tarball;
mod url;
mod workspace;

/**
 * Contains the information we keep in the lockfile for a given package.
 */
#[derive(Clone, Debug, Deserialize, Decode, Encode, Serialize, PartialEq, Eq)]
pub struct Resolution {
    #[serde(rename = "resolution")]
    pub locator: Locator,
    pub version: crate::semver::Version,

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

impl IntoResolutionResult for Resolution {
    fn into_resolution_result(mut self, context: &InstallContext<'_>) -> ResolutionResult {
        let original_resolution = self.clone();

        let (dependencies, peer_dependencies)
            = normalize_resolutions(context, &self);

        self.dependencies = dependencies;
        self.peer_dependencies = peer_dependencies;

        ResolutionResult {
            resolution: self,
            original_resolution,
            package_data: None,
        }
    }
}

pub async fn resolve_descriptor(context: InstallContext<'_>, descriptor: Descriptor, dependencies: Vec<InstallOpResult>) -> Result<ResolutionResult, Error> {
    match &descriptor.range {
        Range::AnonymousSemver(params)
            => semver::resolve_descriptor(&context, &descriptor, params).await,

        Range::AnonymousTag(params)
            => tag::resolve_descriptor(&context, &descriptor, params).await,

        Range::Git(params)
            => git::resolve_descriptor(&context, &descriptor, params).await,

        Range::RegistrySemver(params)
            => npm::resolve_semver_descriptor(&context, &descriptor, params).await,

        Range::Link(params)
            => link::resolve_descriptor(&context, &descriptor, params),

        Range::Url(params)
            => url::resolve_descriptor(&context, &descriptor, params).await,

        Range::Patch(params)
            => patch::resolve_descriptor(&context, &descriptor, params, dependencies).await,

        Range::Tarball(params)
            => tarball::resolve_descriptor(&context, &descriptor, params, dependencies).await,

        Range::Folder(params)
            => folder::resolve_descriptor(&context, &descriptor, params, dependencies).await,

        Range::Portal(params)
            => portal::resolve_descriptor(&context, &descriptor, params, dependencies),

        Range::RegistryTag(params)
            => npm::resolve_tag_descriptor(&context, &descriptor, params).await,

        Range::WorkspaceMagic(_)
            => workspace::resolve_name_descriptor(&context, &descriptor, &range::WorkspaceIdentRange {ident: descriptor.ident.clone()}),

        Range::WorkspaceSemver(_)
            => workspace::resolve_name_descriptor(&context, &descriptor, &range::WorkspaceIdentRange {ident: descriptor.ident.clone()}),

        Range::WorkspacePath(params)
            => workspace::resolve_path_descriptor(&context, &descriptor, params),

        _ => Err(Error::Unsupported),
    }
}

pub async fn resolve_locator<'a>(context: InstallContext<'a>, locator: &Locator, dependencies: Vec<InstallOpResult>) -> Result<ResolutionResult, Error> {
    match &locator.reference {
        Reference::Link(params)
            => link::resolve_locator(&context, locator, params),

        Reference::Portal(params)
            => portal::resolve_locator(&context, locator, params, dependencies),

        Reference::Url(params)
            => url::resolve_locator(&context, locator, params).await,

        Reference::Tarball(params)
            => tarball::resolve_locator(&context, locator, params, dependencies).await,

        Reference::Folder(params)
            => folder::resolve_locator(&context, locator, params, dependencies).await,

        Reference::Git(params)
            => git::resolve_locator(&context, locator, params).await,

        Reference::Patch(params)
            => patch::resolve_locator(&context, locator, params, dependencies).await,

        Reference::Registry(params)
            => npm::resolve_locator(&context, locator, params).await,

        Reference::Workspace(params)
            => workspace::resolve_locator(&context, locator, params),

        _ => Err(Error::Unsupported),
    }
}
