//! `Cargo.toml` as a dependency manifest.
//!
//! This is what lets `yarn add`, `yarn up` and `yarn remove` work on
//! crates: they go through `manifest::dependencies`, which hands them the
//! `Cargo.toml` files of the workspaces belonging to a cargo island for
//! everything using the `cargo:` protocol. Edits keep the formatting and
//! the comments of the files.

use std::collections::BTreeSet;

use toml_edit::{DocumentMut, InlineTable, Item, TableLike, Value};
use zpm_primitives::{CargoGitRange, CargoGitRef, CargoPathRange, CargoRange, CargoRequirement, Descriptor, Ident, Range};
use zpm_semver::RangeKind;
use zpm_utils::{FromFileString, Path, ToFileString};

use crate::{
    error::Error,
    install::InstallContext,
    manifest::dependencies::{DeclaredDependency, DependencyField, DependencyManifest, ManifestScope},
    project::{Project, Workspace},
};

use super::{
    CargoContext,
    DependencyKind,
    FeatureValue,
    Registries,
    RegistryIndex,
    index,
    manifest::{DEPENDENCY_TABLES, DependencySpec, MANIFEST_NAME, Manifest},
};

pub fn is_cargo_range(range: &Range) -> bool {
    matches!(range, Range::Cargo(_) | Range::CargoGit(_) | Range::CargoPath(_))
}

/// The requirement `yarn add` and `yarn up` write for the latest release
/// of a crate. Cargo's own convention is followed: a bare version, which
/// is a caret requirement.
pub async fn latest_requirement(context: &InstallContext<'_>, registry: &RegistryIndex, name: &str, range_kind: RangeKind) -> Result<CargoRequirement, Error> {
    let project
        = context.project
            .expect("The project is required for resolving crates");

    let versions
        = index::fetch_crate(&CargoContext::new(project), registry, name, None).await?;

    let latest = versions.versions.iter().rev()
        .find(|summary| !summary.yanked && !summary.version.is_prerelease())
        .or_else(|| versions.versions.iter().rev().find(|summary| !summary.yanked))
        .ok_or_else(|| Error::CargoError(format!("The crate {} has no version that can be installed", name)))?;

    let requirement = match range_kind {
        RangeKind::Exact => format!("={}", latest.version),
        RangeKind::Tilde => format!("~{}", latest.version),
        _ => latest.version.to_string(),
    };

    CargoRequirement::from_file_string(&requirement)
        .map_err(|err| Error::CargoError(err.to_string()))
}

/// The tables of a manifest holding dependencies, as paths from its root.
fn dependency_tables(document: &DocumentMut) -> Vec<(Vec<String>, DependencyKind)> {
    let mut tables
        = Vec::new();

    for (name, kind) in DEPENDENCY_TABLES {
        if document.get(name).is_some_and(|item| item.is_table_like()) {
            tables.push((vec![name.to_string()], kind));
        }
    }

    let targets
        = document.get("target").and_then(|item| item.as_table_like());

    for (target, tables_of_target) in targets.into_iter().flat_map(|targets| targets.iter()) {
        for (name, kind) in DEPENDENCY_TABLES {
            if tables_of_target.get(name).is_some_and(|item| item.is_table_like()) {
                tables.push((vec!["target".to_string(), target.to_string(), name.to_string()], kind));
            }
        }
    }

    // What the members of a workspace inherit
    if document.get("workspace").and_then(|item| item.get("dependencies")).is_some_and(|item| item.is_table_like()) {
        tables.push((vec!["workspace".to_string(), "dependencies".to_string()], DependencyKind::Normal));
    }

    tables
}

fn table_at<'a>(document: &'a DocumentMut, path: &[String]) -> Option<&'a dyn TableLike> {
    let mut item
        = document.as_item();

    for key in path {
        item = item.get(key)?;
    }

    item.as_table_like()
}

fn table_at_mut<'a>(document: &'a mut DocumentMut, path: &[String]) -> Option<&'a mut dyn TableLike> {
    let mut item
        = document.as_item_mut();

    for key in path {
        item = item.get_mut(key)?;
    }

    item.as_table_like_mut()
}

/// What writing a dependency entry takes to know about the manifest
/// holding it.
struct EntryLocation<'a> {
    /// The folder of the manifest
    dir: &'a Path,
    project_cwd: &'a Path,
    registries: &'a Registries,
}

/// Writes what a range designates in a dependency table entry, leaving
/// the rest of the entry (features, `optional`, ...) as it is.
fn apply_range(entry: &mut dyn TableLike, range: &Range, location: &EntryLocation<'_>) -> Result<(), Error> {
    let alias = match range {
        Range::Cargo(params) => &params.ident,
        Range::CargoGit(params) => &params.ident,
        Range::CargoPath(params) => &params.ident,
        _ => return Err(Error::CargoError(format!("{} can't be listed in a Cargo.toml", range.to_file_string()))),
    };

    let mut fields: Vec<(&str, String)>
        = Vec::new();

    if let Some(alias) = alias {
        fields.push(("package", alias.as_str().to_string()));
    }

    match range {
        Range::Cargo(params) => {
            fields.push(("version", params.requirement.as_str().to_string()));

            // Manifests refer to registries by name when they have one
            if let Some(index) = &params.registry {
                match location.registries.names.iter().find(|(_, candidate)| *candidate == index) {
                    Some((name, _)) => fields.push(("registry", name.clone())),
                    None => fields.push(("registry-index", format!("sparse+{}", index))),
                }
            }
        },

        Range::CargoGit(params) => {
            fields.push(("git", params.source.url().to_string()));

            match params.source.git_ref() {
                CargoGitRef::DefaultBranch => {},
                CargoGitRef::Branch(branch) => fields.push(("branch", branch)),
                CargoGitRef::Tag(tag) => fields.push(("tag", tag)),
                CargoGitRef::Rev(rev) => fields.push(("rev", rev)),
            }
        },

        Range::CargoPath(params) => {
            // Ranges are relative to the project, manifests to themselves
            let target
                = location.project_cwd.with_join_str(&params.path);

            fields.push(("path", target.relative_to(location.dir).to_file_string()));
        },

        _ => unreachable!(),
    }

    // The fields saying where the crate comes from are replaced; the ones
    // that stay keep their place in the entry
    for key in ["version", "git", "branch", "tag", "rev", "path", "registry", "registry-index", "package"] {
        if !fields.iter().any(|(field, _)| *field == key) {
            entry.remove(key);
        }
    }

    for (key, value) in fields {
        match entry.get_mut(key) {
            Some(item) => *item = toml_edit::value(value),
            None => {entry.insert(key, toml_edit::value(value));},
        }
    }

    Ok(())
}

/// Changes the value of a `name = "1.2.3"` entry, keeping the comment
/// that may follow it.
fn set_requirement(entry: &mut Item, requirement: &str) {
    let decor
        = entry.as_value().map(|value| value.decor().clone());

    let mut value
        = Value::from(requirement);

    if let Some(decor) = decor {
        *value.decor_mut() = decor;
    }

    *entry = Item::Value(value);
}

/// A `Cargo.toml`, along with the root of its workspace when some of its
/// dependencies are inherited from it.
pub struct CargoManifestEditor {
    path: Path,
    dir: Path,
    document: DocumentMut,
    original: String,
    workspace: Option<Box<CargoManifestEditor>>,
    project_cwd: Path,
    registries: Registries,
}

impl CargoManifestEditor {
    fn from_manifest(manifest: Manifest, project_cwd: &Path, registries: &Registries) -> CargoManifestEditor {
        CargoManifestEditor {
            path: manifest.dir.with_join_str(MANIFEST_NAME),
            dir: manifest.dir,
            document: manifest.document,
            original: manifest.text,
            workspace: None,
            project_cwd: project_cwd.clone(),
            registries: registries.clone(),
        }
    }

    pub fn open(dir: &Path, project_cwd: &Path, registries: &Registries) -> Result<CargoManifestEditor, Error> {
        let manifest
            = Manifest::read(dir)?;

        let workspace
            = manifest.find_workspace_root(None)?
                .map(|workspace| Box::new(CargoManifestEditor::from_manifest(workspace, project_cwd, registries)));

        let mut editor
            = CargoManifestEditor::from_manifest(manifest, project_cwd, registries);

        editor.workspace = workspace;

        Ok(editor)
    }

    /// The range a dependency table entry stands for.
    fn range_of(&self, name: &str, item: &Item) -> Option<Range> {
        let spec
            = DependencySpec::from_item(item, &self.dir)?;

        if spec.inherited {
            let workspace
                = self.workspace.as_ref()?;

            let inherited
                = table_at(&workspace.document, &["workspace".to_string(), "dependencies".to_string()])?.get(name)?;

            return workspace.range_of(name, inherited);
        }

        let alias = spec.package.as_ref()
            .filter(|package| package.as_str() != name)
            .map(Ident::new);

        if let Some(path) = &spec.path {
            return Some(CargoPathRange {
                ident: alias,
                path: path.relative_to(&self.project_cwd).to_file_string(),
            }.into());
        }

        if let Some(source) = spec.git {
            return Some(CargoGitRange {ident: alias, source}.into());
        }

        let registry = match (&spec.registry, &spec.registry_index) {
            (Some(name), _) => self.registries.by_name(name)?,
            (None, Some(index)) => self.registries.from_url(index),
            (None, None) => None,
        };

        Some(CargoRange {
            ident: alias,
            requirement: CargoRequirement::from_file_string(spec.version.as_deref().unwrap_or("*")).ok()?,
            registry,
        }.into())
    }

    fn is_inherited(item: &Item) -> bool {
        item.as_table_like()
            .and_then(|table| table.get("workspace"))
            .and_then(|item| item.as_bool())
            .unwrap_or(false)
    }
}

impl DependencyManifest for CargoManifestEditor {
    fn path(&self) -> &Path {
        &self.path
    }

    fn accepts(&self, range: &Range) -> bool {
        is_cargo_range(range)
    }

    fn dependencies(&self) -> Vec<DeclaredDependency> {
        let mut dependencies
            = Vec::new();

        for (path, kind) in dependency_tables(&self.document) {
            let Some(table) = table_at(&self.document, &path) else {
                continue;
            };

            for (name, item) in table.iter() {
                let Some(range) = self.range_of(name, item) else {
                    continue;
                };

                let is_optional = item.as_table_like()
                    .and_then(|entry| entry.get("optional"))
                    .and_then(|optional| optional.as_bool())
                    .unwrap_or(false);

                let field = match (kind, is_optional) {
                    (DependencyKind::Normal, true) => DependencyField::Optional,
                    (DependencyKind::Normal, false) => DependencyField::Regular,
                    (DependencyKind::Build, _) => DependencyField::Build,
                    (DependencyKind::Development, _) => DependencyField::Dev,
                };

                dependencies.push(DeclaredDependency {
                    field,
                    descriptor: Descriptor::new(Ident::new(name), range),
                });
            }
        }

        dependencies
    }

    fn set(&mut self, field: DependencyField, descriptor: &Descriptor) -> Result<(), Error> {
        if self.document.get("package").is_none() {
            return Err(Error::CargoError(format!("{} doesn't declare a package; dependencies are added to the crates of the workspace", self.path.to_file_string())));
        }

        let table_name = match field {
            DependencyField::Regular | DependencyField::Optional => "dependencies",
            DependencyField::Dev => "dev-dependencies",
            DependencyField::Build => "build-dependencies",

            DependencyField::Peer => {
                return Err(Error::ConflictingOptions("Crates don't have peer dependencies".to_string()));
            },
        };

        let name
            = descriptor.ident.as_str();

        let existing
            = self.document.get(table_name).and_then(|table| table.get(name)).cloned();

        // An inherited dependency is defined by the root of the workspace
        if existing.as_ref().is_some_and(Self::is_inherited) {
            if let Some(workspace) = &mut self.workspace {
                let location = EntryLocation {
                    dir: &workspace.dir,
                    project_cwd: &self.project_cwd,
                    registries: &self.registries,
                };

                let entry = table_at_mut(&mut workspace.document, &["workspace".to_string(), "dependencies".to_string()])
                    .and_then(|table| table.get_mut(name));

                if let Some(entry) = entry {
                    return set_entry(entry, &descriptor.range, false, &location);
                }
            }
        }

        let location = EntryLocation {
            dir: &self.dir,
            project_cwd: &self.project_cwd,
            registries: &self.registries,
        };

        let mut entry
            = existing.unwrap_or(Item::None);

        set_entry(&mut entry, &descriptor.range, field == DependencyField::Optional, &location)?;

        if self.document.get(table_name).is_none() {
            self.document.insert(table_name, toml_edit::table());
        }

        let table
            = self.document.get_mut(table_name).and_then(|item| item.as_table_like_mut())
                .ok_or_else(|| Error::CargoError(format!("`{}` isn't a table in {}", table_name, self.path.to_file_string())))?;

        // New entries go where Cargo would put them: at their place in
        // tables that are sorted, at the end of the others
        let was_sorted
            = table.iter().map(|(key, _)| key.to_string()).is_sorted();

        table.insert(name, entry);

        if was_sorted {
            table.sort_values();
        }

        Ok(())
    }

    fn update(&mut self, descriptor: &Descriptor) -> Result<bool, Error> {
        let Range::Cargo(params) = &descriptor.range else {
            return Ok(false);
        };

        let name
            = descriptor.ident.as_str();

        let mut is_listed
            = false;

        for (path, _) in dependency_tables(&self.document) {
            let Some(current) = table_at(&self.document, &path).and_then(|table| table.get(name)) else {
                continue;
            };

            // Inherited entries are updated where they're defined, and
            // only registry dependencies have a newer release to move to
            let is_registry_dependency
                = !Self::is_inherited(current) && matches!(self.range_of(name, current), Some(Range::Cargo(_)));

            if !is_registry_dependency {
                continue;
            }

            is_listed = true;

            let Some(entry) = table_at_mut(&mut self.document, &path).and_then(|table| table.get_mut(name)) else {
                continue;
            };

            match entry.as_table_like_mut().and_then(|table| table.get_mut("version")) {
                Some(version) => set_requirement(version, params.requirement.as_str()),
                None => set_requirement(entry, params.requirement.as_str()),
            }
        }

        Ok(is_listed)
    }

    fn remove(&mut self, ident: &Ident) -> Result<bool, Error> {
        let name
            = ident.as_str();

        let mut was_listed
            = false;

        for (path, _) in dependency_tables(&self.document) {
            // What the members inherit stays, as others may rely on it
            if path.first().is_some_and(|key| key == "workspace") {
                continue;
            }

            let Some(table) = table_at_mut(&mut self.document, &path) else {
                continue;
            };

            if table.remove(name).is_none() {
                continue;
            }

            was_listed = true;

            // A table left empty goes away with its last entry
            if table.is_empty() {
                let (key, parent)
                    = path.split_last().unwrap();

                if let Some(parent) = table_at_mut(&mut self.document, parent) {
                    parent.remove(key);
                }
            }
        }

        // Features can't refer to a dependency that's gone
        if was_listed {
            if let Some(features) = self.document.get_mut("features").and_then(|item| item.as_table_like_mut()) {
                for (_, values) in features.iter_mut() {
                    if let Some(values) = values.as_array_mut() {
                        values.retain(|value| value.as_str().is_none_or(|value| match FeatureValue::parse(value) {
                            FeatureValue::Feature(feature) => feature != name,
                            FeatureValue::Dependency(dependency) => dependency != name,
                            FeatureValue::DependencyFeature {dependency, ..} => dependency != name,
                        }));

                        // What was the second value mustn't keep the space
                        // separating it from the first
                        if let Some(first) = values.get_mut(0) {
                            if first.decor().prefix().and_then(|prefix| prefix.as_str()) == Some(" ") {
                                first.decor_mut().set_prefix("");
                            }
                        }
                    }
                }
            }
        }

        Ok(was_listed)
    }

    fn save(&mut self) -> Result<(), Error> {
        let text
            = self.document.to_string();

        if text != self.original {
            self.path.fs_write_text(&text)?;
            self.original = text;
        }

        if let Some(workspace) = &mut self.workspace {
            workspace.save()?;
        }

        Ok(())
    }
}

/// Writes a range in a dependency entry, creating it in its simplest form
/// (`name = "1.2.3"`) when it doesn't exist yet.
fn set_entry(entry: &mut Item, range: &Range, optional: bool, location: &EntryLocation<'_>) -> Result<(), Error> {
    let is_plain_requirement
        = matches!(range, Range::Cargo(params) if params.ident.is_none() && params.registry.is_none());

    if let (true, false, Range::Cargo(params)) = (is_plain_requirement, optional, range) {
        if !entry.is_table_like() {
            set_requirement(entry, params.requirement.as_str());
            return Ok(());
        }
    }

    // `name = "1.2.3"` becomes `name = { version = "1.2.3", ... }`
    if !entry.is_table_like() {
        *entry = Item::Value(Value::InlineTable(InlineTable::new()));
    }

    let table
        = entry.as_table_like_mut().unwrap();

    apply_range(table, range, location)?;

    if optional {
        table.insert("optional", toml_edit::value(true));
    }

    if let Some(inline) = entry.as_inline_table_mut() {
        inline.fmt();
    }

    Ok(())
}

/// The crate the current directory designates: the closest folder, up to
/// the workspace's, holding the manifest of a package.
fn find_active_crate(project: &Project, workspace: &Workspace) -> Result<Option<Path>, Error> {
    let mut current
        = Some(project.project_cwd.with_join(&project.shell_cwd));

    while let Some(dir) = current {
        if !workspace.path.contains(&dir) {
            break;
        }

        if Manifest::exists(&dir) && Manifest::read(&dir)?.package().is_some() {
            return Ok(Some(dir));
        }

        current = dir.dirname().filter(|parent| parent != &dir);
    }

    Ok(None)
}

/// Opens the `Cargo.toml` files of a workspace. The inner error is the
/// reason why none can be picked, for the commands that need exactly one.
pub fn open_manifests(project: &Project, workspace: &Workspace, scope: ManifestScope) -> Result<Result<Vec<CargoManifestEditor>, String>, Error> {
    if !Manifest::exists(&workspace.path) {
        return Ok(Ok(vec![]));
    }

    let registries
        = Registries::new(project);

    match scope {
        ManifestScope::Active => {
            let Some(dir) = find_active_crate(project, workspace)? else {
                return Ok(Err(format!(
                    "{} is the root of a Cargo workspace rather than a crate; run this command from the folder of the crate to change",
                    workspace.path.with_join_str(MANIFEST_NAME).to_file_string(),
                )));
            };

            Ok(Ok(vec![CargoManifestEditor::open(&dir, &project.project_cwd, &registries)?]))
        },

        ManifestScope::All => {
            let root
                = Manifest::read(&workspace.path)?;

            let dirs = root.member_dirs()?.into_iter()
                .chain([workspace.path.clone()])
                .collect::<BTreeSet<_>>();

            // Each file is opened on its own, so that two editors never
            // hold diverging copies of the root of the workspace
            dirs.into_iter()
                .map(|dir| Ok(CargoManifestEditor::from_manifest(Manifest::read(&dir)?, &project.project_cwd, &registries)))
                .collect::<Result<Vec<_>, Error>>()
                .map(Ok)
        },
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    const ROOT: &str = r#"[workspace]
members = ["crates/*"]

[workspace.dependencies]
serde = { version = "1.0.200", features = ["derive"] }
"#;

    const MEMBER: &str = r#"[package]
name = "app"
version = "0.1.0"

# Runtime dependencies
[dependencies]
anyhow = "1.0.80"  # errors
serde = { workspace = true }
tokio = { version = "1.39", features = ["full"] }
upstream = { git = "https://example.com/upstream.git" }

[target.'cfg(unix)'.dependencies]
libc = "0.2.150"

[dev-dependencies]
rstest = "0.26"

[features]
default = ["tokio/macros"]
async = ["dep:tokio", "anyhow"]
"#;

    fn registries() -> Registries {
        Registries {
            default_index: "https://index.crates.io/".to_string(),
            names: BTreeMap::from([("internal".to_string(), "https://crates.example.org/index/".to_string())]),
            tokens: BTreeMap::new(),
        }
    }

    fn editor() -> CargoManifestEditor {
        let project_cwd
            = Path::from_file_string("/repo").unwrap();

        let registries
            = registries();

        let member
            = Manifest::parse(Path::from_file_string("/repo/crates/app").unwrap(), MEMBER.to_string()).unwrap();
        let root
            = Manifest::parse(project_cwd.clone(), ROOT.to_string()).unwrap();

        let mut editor
            = CargoManifestEditor::from_manifest(member, &project_cwd, &registries);

        editor.workspace = Some(Box::new(CargoManifestEditor::from_manifest(root, &project_cwd, &registries)));
        editor
    }

    fn descriptor(src: &str) -> Descriptor {
        Descriptor::from_file_string(src).unwrap()
    }

    #[test]
    fn test_listing() {
        let dependencies = editor().dependencies().into_iter()
            .map(|dependency| format!("{} {}", dependency.field.label(), dependency.descriptor.to_file_string()))
            .collect::<Vec<_>>();

        assert_eq!(dependencies, vec![
            "regular anyhow@cargo:1.0.80",
            // Inherited dependencies are read from where they're defined
            "regular serde@cargo:1.0.200",
            "regular tokio@cargo:1.39",
            "regular upstream@cargo:git+https://example.com/upstream.git",
            "dev rstest@cargo:0.26",
            "regular libc@cargo:0.2.150",
        ]);
    }

    #[test]
    fn test_adding() {
        let mut editor
            = editor();

        editor.set(DependencyField::Regular, &descriptor("itoa@cargo:1.0.11")).unwrap();
        editor.set(DependencyField::Regular, &descriptor("json@cargo:serde_json@1")).unwrap();
        editor.set(DependencyField::Optional, &descriptor("zstd@cargo:0.13")).unwrap();
        editor.set(DependencyField::Dev, &descriptor("private@cargo:2#https://crates.example.org/index/")).unwrap();
        editor.set(DependencyField::Build, &descriptor("cc@cargo:1")).unwrap();
        editor.set(DependencyField::Regular, &descriptor("sibling@cargo:path+crates/sibling")).unwrap();
        editor.set(DependencyField::Regular, &descriptor("fork@cargo:git+https://example.com/fork.git?branch=fix")).unwrap();

        let text
            = editor.document.to_string();

        // The table was sorted, and stays so; what was there is untouched
        assert!(text.contains(r#"# Runtime dependencies
[dependencies]
anyhow = "1.0.80"  # errors
fork = { git = "https://example.com/fork.git", branch = "fix" }
itoa = "1.0.11"
json = { package = "serde_json", version = "1" }
serde = { workspace = true }
sibling = { path = "../sibling" }
tokio = { version = "1.39", features = ["full"] }
upstream = { git = "https://example.com/upstream.git" }
zstd = { version = "0.13", optional = true }
"#), "{}", text);

        assert!(text.contains("[dev-dependencies]\nprivate = { version = \"2\", registry = \"internal\" }\nrstest = \"0.26\"\n"), "{}", text);
        assert!(text.contains("[build-dependencies]\ncc = \"1\"\n"), "{}", text);

        assert!(editor.set(DependencyField::Peer, &descriptor("itoa@cargo:1")).is_err());
    }

    #[test]
    fn test_changing_existing_entries() {
        let mut editor
            = editor();

        // The features of an entry survive the change of its version
        editor.set(DependencyField::Regular, &descriptor("tokio@cargo:1.40")).unwrap();

        // A registry dependency can be pointed to a repository, and back
        editor.set(DependencyField::Regular, &descriptor("upstream@cargo:3.1")).unwrap();

        // Inherited dependencies are changed at the root of the workspace
        editor.set(DependencyField::Regular, &descriptor("serde@cargo:1.0.229")).unwrap();

        let text
            = editor.document.to_string();

        assert!(text.contains("tokio = { version = \"1.40\", features = [\"full\"] }"), "{}", text);
        assert!(text.contains("upstream = { version = \"3.1\" }"), "{}", text);
        assert!(text.contains("serde = { workspace = true }"), "{}", text);

        let root
            = editor.workspace.as_ref().unwrap().document.to_string();

        assert!(root.contains("serde = { version = \"1.0.229\", features = [\"derive\"] }"), "{}", root);
    }

    #[test]
    fn test_updating() {
        let mut editor
            = editor();

        assert!(editor.update(&descriptor("anyhow@cargo:1.0.99")).unwrap());
        assert!(editor.update(&descriptor("libc@cargo:0.2.170")).unwrap());
        assert!(editor.update(&descriptor("tokio@cargo:1.47")).unwrap());

        // Not listed, defined elsewhere, or not from a registry
        assert!(!editor.update(&descriptor("missing@cargo:1")).unwrap());
        assert!(!editor.update(&descriptor("serde@cargo:1.0.229")).unwrap());
        assert!(!editor.update(&descriptor("upstream@cargo:2")).unwrap());

        let text
            = editor.document.to_string();

        assert!(text.contains("anyhow = \"1.0.99\"  # errors\n"), "{}", text);
        assert!(text.contains("libc = \"0.2.170\"\n"), "{}", text);
        assert!(text.contains("tokio = { version = \"1.47\", features = [\"full\"] }"), "{}", text);
        assert!(text.contains("upstream = { git = \"https://example.com/upstream.git\" }"), "{}", text);

        // The root of a workspace is where inherited dependencies move
        let mut root
            = *editor.workspace.take().unwrap();

        assert!(root.update(&descriptor("serde@cargo:1.0.229")).unwrap());
        assert!(root.document.to_string().contains("serde = { version = \"1.0.229\", features = [\"derive\"] }"));
    }

    #[test]
    fn test_removing() {
        let mut editor
            = editor();

        assert!(editor.remove(&Ident::new("tokio")).unwrap());
        assert!(editor.remove(&Ident::new("libc")).unwrap());
        assert!(!editor.remove(&Ident::new("missing")).unwrap());

        let text
            = editor.document.to_string();

        assert!(!text.contains("tokio"), "{}", text);
        assert!(!text.contains("libc"), "{}", text);

        // ... and so are the tables left empty
        assert!(!text.contains("cfg(unix)"), "{}", text);
        assert!(text.contains("[dev-dependencies]\nrstest = \"0.26\"\n"), "{}", text);

        // Features referring to the dependency are cleaned up
        assert!(text.contains("default = []"), "{}", text);
        assert!(text.contains("async = [\"anyhow\"]"), "{}", text);
    }
}
