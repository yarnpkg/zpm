//! Where the dependencies of a workspace are declared.
//!
//! `package.json` isn't the only file holding dependencies: the crates of
//! a Rust workspace are listed in `Cargo.toml` files, and other ecosystems
//! have their own manifests. The commands editing dependencies (`yarn add`,
//! `yarn up`, `yarn remove`) don't write to a given file; they go through
//! the manifests of the workspace, each of which says which dependencies
//! are its own.
//!
//! Supporting another kind of manifest takes an implementation of
//! `DependencyManifest`, and returning it from `companion_manifests`.

use zpm_config::IslandLinker;
use zpm_parsers::{Document, JsonDocument, Value};
use zpm_primitives::{Descriptor, Ident, Range};
use zpm_utils::{FromFileString, Path, ToFileString};

use crate::{
    error::Error,
    install::InstallContext,
    project::{MANIFEST_NAME, Project, Workspace},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DependencyField {
    Regular,
    Dev,
    Optional,
    Peer,

    /// Dependencies of the build scripts; only some ecosystems have them
    Build,
}

impl DependencyField {
    pub fn label(&self) -> &'static str {
        match self {
            DependencyField::Regular => "regular",
            DependencyField::Dev => "dev",
            DependencyField::Optional => "optional",
            DependencyField::Peer => "peer",
            DependencyField::Build => "build",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeclaredDependency {
    pub field: DependencyField,
    pub descriptor: Descriptor,
}

/// A file declaring dependencies.
pub trait DependencyManifest {
    /// The file, for messages
    fn path(&self) -> &Path;

    /// Whether dependencies using this range are declared in this file.
    fn accepts(&self, range: &Range) -> bool;

    fn dependencies(&self) -> Vec<DeclaredDependency>;

    /// Declares a dependency, or changes the one already listed in that
    /// field.
    fn set(&mut self, field: DependencyField, descriptor: &Descriptor) -> Result<(), Error>;

    /// Changes the range of a dependency wherever the file lists it;
    /// returns whether it does.
    fn update(&mut self, descriptor: &Descriptor) -> Result<bool, Error>;

    /// Removes a dependency from every field; returns whether it was listed.
    fn remove(&mut self, ident: &Ident) -> Result<bool, Error>;

    fn save(&mut self) -> Result<(), Error>;
}

/// Whether dependencies using this range are declared in a manifest of
/// their own ecosystem rather than in `package.json`.
fn has_dedicated_manifest(range: &Range) -> bool {
    crate::cargo::editor::is_cargo_range(range)
}

/// The `package.json` of a workspace.
pub struct PackageManifest {
    path: Path,
    document: JsonDocument,
    dependencies: Vec<DeclaredDependency>,
}

const PACKAGE_FIELDS: [(DependencyField, &str); 4] = [
    (DependencyField::Regular, "dependencies"),
    (DependencyField::Dev, "devDependencies"),
    (DependencyField::Optional, "optionalDependencies"),
    (DependencyField::Peer, "peerDependencies"),
];

impl PackageManifest {
    pub fn open(workspace: &Workspace) -> Result<PackageManifest, Error> {
        let path
            = workspace.path.with_join_str(MANIFEST_NAME);

        let document
            = JsonDocument::new(path.fs_read_prealloc()?)?;

        let manifest
            = &workspace.manifest;

        let peer_dependencies = manifest.remote.peer_dependencies.iter()
            .filter_map(|(ident, range)| Some(Descriptor::new(ident.clone(), Range::from_file_string(&range.to_file_string()).ok()?)));

        let dependencies = manifest.remote.dependencies.values().map(|descriptor| (DependencyField::Regular, descriptor.clone()))
            .chain(manifest.dev_dependencies.values().map(|descriptor| (DependencyField::Dev, descriptor.clone())))
            .chain(manifest.remote.optional_dependencies.values().map(|descriptor| (DependencyField::Optional, descriptor.clone())))
            .chain(peer_dependencies.map(|descriptor| (DependencyField::Peer, descriptor)))
            .map(|(field, descriptor)| DeclaredDependency {field, descriptor})
            .collect();

        Ok(PackageManifest {path, document, dependencies})
    }

    fn field_path(field: &str, ident: &Ident) -> zpm_parsers::Path {
        zpm_parsers::Path::from_segments(vec![field.to_string(), ident.to_file_string()])
    }
}

impl DependencyManifest for PackageManifest {
    fn path(&self) -> &Path {
        &self.path
    }

    fn accepts(&self, range: &Range) -> bool {
        !has_dedicated_manifest(range)
    }

    fn dependencies(&self) -> Vec<DeclaredDependency> {
        self.dependencies.clone()
    }

    fn set(&mut self, field: DependencyField, descriptor: &Descriptor) -> Result<(), Error> {
        let Some((_, name)) = PACKAGE_FIELDS.iter().find(|(candidate, _)| *candidate == field) else {
            return Err(Error::ConflictingOptions(format!("{} can't hold {} dependencies", MANIFEST_NAME, field.label())));
        };

        // Peer dependencies accept any version unless they refer to a
        // catalog, which is resolved into the catalog's range at install
        let range = match (field, &descriptor.range) {
            (DependencyField::Peer, Range::Catalog(_)) => descriptor.range.to_file_string(),
            (DependencyField::Peer, _) => "*".to_string(),
            _ => descriptor.range.to_anonymous_range().to_file_string(),
        };

        self.document.set_path(&Self::field_path(name, &descriptor.ident), Value::String(range))?;

        Ok(())
    }

    fn update(&mut self, descriptor: &Descriptor) -> Result<bool, Error> {
        let range
            = descriptor.range.to_anonymous_range().to_file_string();

        let mut is_listed
            = false;

        for (field, name) in PACKAGE_FIELDS {
            if field == DependencyField::Peer {
                continue;
            }

            is_listed |= self.dependencies.iter().any(|dependency| dependency.field == field && dependency.descriptor.ident == descriptor.ident);

            self.document.update_path(&Self::field_path(name, &descriptor.ident), Value::String(range.clone()))?;
        }

        Ok(is_listed)
    }

    fn remove(&mut self, ident: &Ident) -> Result<bool, Error> {
        for (_, name) in PACKAGE_FIELDS {
            self.document.set_path(&Self::field_path(name, ident), Value::Undefined)?;
        }

        Ok(self.dependencies.iter().any(|dependency| &dependency.descriptor.ident == ident))
    }

    fn save(&mut self) -> Result<(), Error> {
        self.path.fs_change(&self.document.input, false)?;

        Ok(())
    }
}

/// Which of a workspace's manifests a command works on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManifestScope {
    /// The manifests the current directory designates (`yarn add`)
    Active,

    /// Every manifest of the workspace (`yarn up`)
    All,
}

/// The manifests of a workspace, besides its `package.json`, along with
/// the ranges an ecosystem owns but has no manifest to put in right now.
struct Companions {
    manifests: Vec<Box<dyn DependencyManifest>>,
    unavailable: Vec<(fn(&Range) -> bool, String)>,
}

fn companion_manifests(project: &Project, workspace: &Workspace, scope: ManifestScope) -> Result<Companions, Error> {
    let mut companions = Companions {
        manifests: vec![],
        unavailable: vec![],
    };

    if project.try_island_by_rel_path(&workspace.rel_path, IslandLinker::Cargo).is_some_and(|island_workspace| island_workspace.name == workspace.name) {
        match crate::cargo::editor::open_manifests(project, workspace, scope)? {
            Ok(manifests) => {
                companions.manifests.extend(manifests.into_iter().map(|manifest| Box::new(manifest) as Box<dyn DependencyManifest>));
            },

            Err(reason) => {
                companions.unavailable.push((crate::cargo::editor::is_cargo_range, reason));
            },
        }
    }

    // Python workspaces declaring their dependencies in a pyproject.toml
    // would be plugged here

    Ok(companions)
}

/// The manifests the dependencies of a workspace are declared in.
pub struct ManifestSet {
    manifests: Vec<Box<dyn DependencyManifest>>,
    unavailable: Vec<(fn(&Range) -> bool, String)>,
}

impl ManifestSet {
    pub fn open(project: &Project, workspace: &Workspace, scope: ManifestScope) -> Result<ManifestSet, Error> {
        let Companions {mut manifests, unavailable}
            = companion_manifests(project, workspace, scope)?;

        manifests.push(Box::new(PackageManifest::open(workspace)?));

        Ok(ManifestSet {manifests, unavailable})
    }

    /// The manifest a dependency using this range is to be declared in.
    pub fn for_range(&mut self, range: &Range) -> Result<&mut dyn DependencyManifest, Error> {
        let Some(index) = self.manifests.iter().position(|manifest| manifest.accepts(range)) else {
            let reason = self.unavailable.iter()
                .find(|(owns, _)| owns(range))
                .map(|(_, reason)| reason.clone())
                .unwrap_or_else(|| format!("No manifest of this workspace can hold {}; is the workspace part of an island using the matching linker?", range.to_file_string()));

            return Err(Error::DependencyManifestError(reason));
        };

        Ok(self.manifests[index].as_mut())
    }

    pub fn iter(&self) -> impl Iterator<Item = &dyn DependencyManifest> {
        self.manifests.iter().map(|manifest| manifest.as_ref())
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut Box<dyn DependencyManifest>> {
        self.manifests.iter_mut()
    }

    /// The manifests that aren't the `package.json`.
    pub fn companions(&self) -> impl Iterator<Item = &dyn DependencyManifest> {
        self.manifests[..self.manifests.len() - 1].iter().map(|manifest| manifest.as_ref())
    }

    pub fn save(&mut self) -> Result<(), Error> {
        for manifest in &mut self.manifests {
            manifest.save()?;
        }

        Ok(())
    }
}

/// The descriptor a dependency declared outside of `package.json` gets
/// once upgraded to the latest release, following the conventions of its
/// ecosystem. Returns `None` for the dependencies that have no notion of
/// latest release (paths, repositories).
pub async fn upgrade_descriptor(context: &InstallContext<'_>, range_kind: zpm_semver::RangeKind, descriptor: &Descriptor) -> Result<Option<Descriptor>, Error> {
    match &descriptor.range {
        Range::Cargo(params) => {
            let name
                = params.ident.as_ref().unwrap_or(&descriptor.ident);

            let requirement
                = crate::cargo::editor::latest_requirement(context, &params.registry, name.as_str(), range_kind).await?;

            Ok(Some(Descriptor::new(descriptor.ident.clone(), zpm_primitives::CargoRange {
                ident: params.ident.clone(),
                requirement,
                registry: params.registry.clone(),
            }.into())))
        },

        _ => Ok(None),
    }
}
