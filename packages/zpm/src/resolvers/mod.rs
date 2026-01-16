use std::collections::{BTreeMap, BTreeSet};

use bincode::{Decode, Encode};
use serde::{Deserialize, Serialize};
use zpm_primitives::{Descriptor, Ident, Locator, PeerRange, Range, Reference, RegistryReference, SemverPeerRange, WorkspaceIdentRange, descriptor_map_serializer, descriptor_map_deserializer};
use zpm_utils::Requirements;

use crate::{
    error::Error, install::{normalize_resolutions, InstallContext, InstallOpResult, IntoResolutionResult, ResolutionResult}, manifest::RemoteManifest
};

pub mod builtin;
pub mod catalog;
pub mod folder;
pub mod git;
pub mod link;
pub mod patch;
pub mod portal;
pub mod npm;
pub mod semver;
pub mod tag;
pub mod tarball;
pub mod url;
pub mod workspace;

/**
 * Contains the information we keep in the lockfile for a given package.
 */
#[derive(Clone, Debug, Deserialize, Decode, Encode, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Resolution {
    #[serde(rename = "resolution")]
    pub locator: Locator,
    pub version: zpm_semver::Version,

    #[serde(default)]
    #[serde(skip_serializing_if = "zpm_utils::is_default")]
    pub requirements: Requirements,

    #[serde(default)]
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    #[serde(serialize_with = "descriptor_map_serializer")]
    #[serde(deserialize_with = "descriptor_map_deserializer")]
    pub dependencies: BTreeMap<Ident, Descriptor>,

    #[serde(default)]
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub peer_dependencies: BTreeMap<Ident, PeerRange>,

    #[serde(default)]
    #[serde(skip_serializing_if = "BTreeSet::is_empty")]
    pub optional_dependencies: BTreeSet<Ident>,

    #[serde(default)]
    #[serde(skip_serializing_if = "BTreeSet::is_empty")]
    pub optional_peer_dependencies: BTreeSet<Ident>,

    #[serde(default)]
    #[serde(skip_serializing_if = "BTreeSet::is_empty")]
    pub missing_peer_dependencies: BTreeSet<Ident>,

    #[serde(default)]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub variants: Vec<Descriptor>,
}

impl Resolution {
    pub fn new_empty(locator: Locator, version: zpm_semver::Version) -> Resolution {
        Resolution {
            locator,
            version,
            requirements: Requirements::default(),
            dependencies: BTreeMap::new(),
            peer_dependencies: BTreeMap::new(),
            optional_dependencies: BTreeSet::new(),
            optional_peer_dependencies: BTreeSet::new(),
            missing_peer_dependencies: BTreeSet::new(),
            variants: Vec::new(),
        }
    }

    pub fn from_remote_manifest(locator: Locator, manifest: RemoteManifest) -> Resolution {
        let optional_dependencies
            = BTreeSet::from_iter(manifest.optional_dependencies.keys().cloned());

        let mut dependencies
            = manifest.dependencies;

        dependencies
            .extend(manifest.optional_dependencies);

        let mut peer_dependencies
            = manifest.peer_dependencies;

        let mut optional_peer_dependencies
            = BTreeSet::new();

        for (ident, meta) in manifest.peer_dependencies_meta {
            peer_dependencies.entry(ident.clone())
                .or_insert(SemverPeerRange {
                    range: zpm_semver::Range::any(),
                }.into());

            if meta.optional {
                optional_peer_dependencies.insert(ident);
            }
        }

        Resolution {
            locator,
            version: manifest.version.unwrap_or_default(),
            dependencies,
            peer_dependencies,
            optional_dependencies,
            optional_peer_dependencies,
            missing_peer_dependencies: BTreeSet::new(),
            requirements: manifest.requirements,
            variants: Vec::new(),
        }
    }
}

impl IntoResolutionResult for Resolution {
    fn into_resolution_result(mut self, context: &InstallContext<'_>) -> Result<ResolutionResult, Error> {
        let original_resolution = self.clone();

        let (dependencies, peer_dependencies)
            = normalize_resolutions(context, &self)?;

        self.dependencies = dependencies;
        self.peer_dependencies = peer_dependencies;

        Ok(ResolutionResult {
            resolution: self,
            original_resolution,
            package_data: None,
        })
    }
}

pub enum SyncResolutionAttempt {
    Success(ResolutionResult),
    Failure(Vec<InstallOpResult>),
}

pub fn try_resolve_descriptor_sync(context: InstallContext<'_>, descriptor: Descriptor, dependencies: Vec<InstallOpResult>) -> Result<SyncResolutionAttempt, Error> {
    match &descriptor.range {
        Range::RegistrySemver(params) if params.ident.is_some()
            => Ok(SyncResolutionAttempt::Success(npm::resolve_aliased(&descriptor, dependencies)?)),

        Range::RegistryTag(params) if params.ident.is_some()
            => Ok(SyncResolutionAttempt::Success(npm::resolve_aliased(&descriptor, dependencies)?)),

        Range::Link(params)
            => Ok(SyncResolutionAttempt::Success(link::resolve_descriptor(&context, &descriptor, params)?)),

        Range::Portal(params)
            => Ok(SyncResolutionAttempt::Success(portal::resolve_descriptor(&context, &descriptor, params, dependencies)?)),

        Range::WorkspaceMagic(_)
            => Ok(SyncResolutionAttempt::Success(workspace::resolve_name_descriptor(&context, &descriptor, &WorkspaceIdentRange {ident: descriptor.ident.clone()})?)),

        Range::WorkspaceSemver(_)
            => Ok(SyncResolutionAttempt::Success(workspace::resolve_name_descriptor(&context, &descriptor, &WorkspaceIdentRange {ident: descriptor.ident.clone()})?)),

        Range::WorkspacePath(params)
            => Ok(SyncResolutionAttempt::Success(workspace::resolve_path_descriptor(&context, &descriptor, params)?)),

        _ => Ok(SyncResolutionAttempt::Failure(dependencies)),
    }
}

pub async fn resolve_descriptor(context: InstallContext<'_>, descriptor: Descriptor, dependencies: Vec<InstallOpResult>) -> Result<ResolutionResult, Error> {
    let project
        = context.project
            .expect("The project is required for resolving a workspace package");

    if let Some(workspace) = project.try_workspace_by_descriptor(&descriptor)? {
        return Ok(workspace::resolve_name_descriptor(&context, &descriptor, &WorkspaceIdentRange {ident: workspace.name.clone()})?);
    }

    match &descriptor.range {
        Range::Builtin(params)
            => builtin::resolve_builtin_descriptor(&context, &descriptor, params).await,

        Range::AnonymousSemver(params)
            => semver::resolve_descriptor(&context, &descriptor, params).await,

        Range::AnonymousTag(params)
            => tag::resolve_descriptor(&context, &descriptor, params).await,

        Range::Git(params)
            => git::resolve_descriptor(&context, &descriptor, params).await,

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

        Range::RegistrySemver(params) => match params.ident.is_some() {
            true => npm::resolve_aliased(&descriptor, dependencies),
            false => npm::resolve_semver_descriptor(&context, &descriptor, params).await,
        },

        Range::RegistryTag(params) => match params.ident.is_some() {
            true => npm::resolve_aliased(&descriptor, dependencies),
            false => npm::resolve_tag_descriptor(&context, &descriptor, params).await,
        },

        Range::WorkspacePath(params)
            => workspace::resolve_path_descriptor(&context, &descriptor, params),

        Range::Catalog(_) |
        Range::MissingPeerDependency |
        Range::WorkspaceMagic(_) |
        Range::WorkspaceSemver(_) |
        Range::WorkspaceIdent(_) |
        Range::Virtual(_) => {
            panic!("Those ranges should never end up being passed to a resolver");
        }
    }
}

pub async fn resolve_locator(context: InstallContext<'_>, locator: Locator, dependencies: Vec<InstallOpResult>) -> Result<ResolutionResult, Error> {
    match &locator.reference {
        Reference::Builtin(params)
            => builtin::resolve_builtin_locator(&context, &locator, &params.version).await,

        Reference::Link(params)
            => link::resolve_locator(&context, &locator, params),

        Reference::Portal(params)
            => portal::resolve_locator(&context, &locator, params, dependencies),

        Reference::Url(params)
            => url::resolve_locator(&context, &locator, params).await,

        Reference::Tarball(params)
            => tarball::resolve_locator(&context, &locator, params, dependencies).await,

        Reference::Folder(params)
            => folder::resolve_locator(&context, &locator, params, dependencies).await,

        Reference::Git(params)
            => git::resolve_locator(&context, &locator, params).await,

        Reference::Patch(params)
            => patch::resolve_locator(&context, &locator, params, dependencies).await,

        Reference::Shorthand(params)
            => npm::resolve_locator(&context, &locator, &RegistryReference {ident: locator.ident.clone(), version: params.version.clone()}).await,

        Reference::Registry(params)
            => npm::resolve_locator(&context, &locator, params).await,

        Reference::Virtual(_)
            => Err(Error::Unsupported)?,

        Reference::WorkspaceIdent(params)
            => workspace::resolve_locator_ident(&context, &locator, params),

        Reference::WorkspacePath(params)
            => workspace::resolve_locator_path(&context, &locator, params),
    }
}

pub async fn validate_resolution(context: InstallContext<'_>, descriptor: Descriptor, locator: Locator, dependencies: Vec<InstallOpResult>) -> Result<(), Error> {
    let success
        = resolve_descriptor(context, descriptor.clone(), dependencies).await?.resolution.locator == locator;

    if !success {
        return Err(Error::BadResolution(descriptor, locator));
    }

    Ok(())
}
