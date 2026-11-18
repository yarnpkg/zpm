//! `Cargo.toml` reader.
//!
//! Only what resolving and vendoring need is read: the package identity,
//! the dependency tables (including the target-specific ones), the features,
//! and the workspace tables members inherit from.

use std::collections::{BTreeMap, BTreeSet};

use toml_edit::{DocumentMut, InlineTable, Item, TableLike, Value};
use zpm_primitives::{CargoGitRef, CargoGitSource, CargoRequirement, CargoSemver};
use zpm_utils::{FromFileString, Path, ToFileString};

use crate::error::Error;

use super::{CrateSource, Dependency, DependencyKind, DependencySource, PatchTarget, Registries, RustVersion, Summary, add_implicit_features, canonical_git_url, parse_rust_version};

pub const MANIFEST_NAME: &str
    = "Cargo.toml";

pub const DEPENDENCY_TABLES: [(&str, DependencyKind); 5] = [
    ("dependencies", DependencyKind::Normal),
    ("build-dependencies", DependencyKind::Build),
    ("build_dependencies", DependencyKind::Build),
    ("dev-dependencies", DependencyKind::Development),
    ("dev_dependencies", DependencyKind::Development),
];

#[derive(Clone, Debug)]
pub struct Manifest {
    /// Absolute path of the folder holding the manifest
    pub dir: Path,
    pub text: String,
    pub document: DocumentMut,
}

fn manifest_error(dir: &Path, message: impl AsRef<str>) -> Error {
    Error::CargoError(format!("{}: {}", dir.with_join_str(MANIFEST_NAME).to_file_string(), message.as_ref()))
}

/// How the path dependencies of a manifest are to be understood.
pub enum PathDependencies<'a> {
    /// The manifest is on the disk; its path dependencies are other crates
    /// on the disk
    Local,

    /// The manifest is part of a git checkout; Cargo treats its path
    /// dependencies as other crates of the same repository
    Git(&'a CargoGitSource),
}

/// What reading the dependencies of a manifest takes besides the manifest.
pub struct ManifestContext<'a> {
    pub path_dependencies: PathDependencies<'a>,
    pub registries: &'a Registries,
}

impl<'a> ManifestContext<'a> {
    pub fn local(registries: &'a Registries) -> Self {
        Self {path_dependencies: PathDependencies::Local, registries}
    }

    pub fn git(source: &'a CargoGitSource, registries: &'a Registries) -> Self {
        Self {path_dependencies: PathDependencies::Git(source), registries}
    }
}

/// A `[replace]` entry: the registry crate it applies to, and what takes
/// its place.
#[derive(Clone, Debug)]
pub struct Replacement {
    pub name: String,
    pub version: CargoSemver,
    pub dependency: Dependency,
}

/// The `name` and `version` of a `[replace]` key (`name:1.2.3`,
/// `name@1.2.3`, or either after `<registry url>#`).
fn parse_replace_key(key: &str) -> Option<(String, CargoSemver)> {
    let spec
        = key.rsplit_once('#').map_or(key, |(_, spec)| spec);

    let (name, version)
        = spec.rsplit_once(['@', ':'])?;

    Some((name.to_string(), CargoSemver::parse(version)?))
}

impl Manifest {
    pub fn read(dir: &Path) -> Result<Manifest, Error> {
        let text
            = dir.with_join_str(MANIFEST_NAME).fs_read_text()
                .map_err(|err| manifest_error(dir, err.to_string()))?;

        Manifest::parse(dir.clone(), text)
    }

    pub fn parse(dir: Path, text: String) -> Result<Manifest, Error> {
        let document
            = text.parse::<DocumentMut>()
                .map_err(|err| manifest_error(&dir, err.to_string()))?;

        Ok(Manifest {dir, text, document})
    }

    pub fn exists(dir: &Path) -> bool {
        dir.with_join_str(MANIFEST_NAME).fs_exists()
    }

    fn table(&self, name: &str) -> Option<&dyn TableLike> {
        self.document.get(name).and_then(|item| item.as_table_like())
    }

    pub fn package(&self) -> Option<&dyn TableLike> {
        self.table("package")
    }

    pub fn workspace(&self) -> Option<&dyn TableLike> {
        self.table("workspace")
    }

    pub fn package_name(&self) -> Option<&str> {
        self.package()?.get("name")?.as_str()
    }

    /// The folders of the workspace members: the root package, and the
    /// folders matched by `workspace.members` (minus `workspace.exclude`).
    pub fn member_dirs(&self) -> Result<Vec<Path>, Error> {
        let mut dirs
            = BTreeSet::new();

        if self.package().is_some() {
            dirs.insert(self.dir.clone());
        }

        let Some(workspace) = self.workspace() else {
            return Ok(dirs.into_iter().collect());
        };

        let patterns = |key: &str| -> Vec<String> {
            workspace.get(key)
                .and_then(|item| item.as_array())
                .map(|array| array.iter().filter_map(|value| value.as_str()).map(|value| value.to_string()).collect())
                .unwrap_or_default()
        };

        let excluded = patterns("exclude").iter()
            .flat_map(|pattern| expand_member_pattern(&self.dir, pattern))
            .collect::<BTreeSet<_>>();

        for pattern in patterns("members") {
            for dir in expand_member_pattern(&self.dir, &pattern) {
                if !excluded.contains(&dir) && Manifest::exists(&dir) {
                    dirs.insert(dir);
                }
            }
        }

        Ok(dirs.into_iter().collect())
    }

    /// The `[patch]` tables: more candidates for the crates of a source,
    /// taken from the disk or from git. Like Cargo, they're only read from
    /// the root of a workspace.
    pub fn patches(&self, context: &ManifestContext<'_>) -> Result<Vec<(PatchTarget, Dependency)>, Error> {
        let mut patches
            = Vec::new();

        let Some(sources) = self.table("patch") else {
            return Ok(patches);
        };

        for (source, entries) in sources.iter() {
            // A source is a registry (by name or by index) or a repository
            let target = match context.registries.by_name(source) {
                Some(registry) => PatchTarget::Registry(registry),

                None if !source.contains("://") => {
                    return Err(manifest_error(&self.dir, format!("[patch.{}] refers to a registry that isn't defined", source)));
                },

                None => match context.registries.names.values().any(|index| *index == super::index::normalize_index_url(source)) {
                    true => PatchTarget::Registry(context.registries.from_url(source)),
                    false => PatchTarget::Git(canonical_git_url(source)),
                },
            };

            for (name, item) in entries.as_table_like().into_iter().flat_map(|table| table.iter()) {
                let dependency
                    = self.parse_dependency(name, item, DependencyKind::Normal, None, None, context)?;

                if matches!(dependency.source, DependencySource::Registry(_)) {
                    return Err(manifest_error(&self.dir, format!("the patch for `{}` must point to a path or to a git repository", name)));
                }

                patches.push((target.clone(), dependency));
            }
        }

        Ok(patches)
    }

    /// The `[replace]` table, which `[patch]` superseded.
    pub fn replacements(&self, context: &ManifestContext<'_>) -> Result<Vec<Replacement>, Error> {
        let mut replacements
            = Vec::new();

        for (key, item) in self.table("replace").into_iter().flat_map(|table| table.iter()) {
            let (name, version)
                = parse_replace_key(key)
                    .ok_or_else(|| manifest_error(&self.dir, format!("`{}` isn't a valid [replace] key; expected `<name>:<version>`", key)))?;

            let dependency
                = self.parse_dependency(&name, item, DependencyKind::Normal, None, None, context)?;

            if matches!(dependency.source, DependencySource::Registry(_)) {
                return Err(manifest_error(&self.dir, format!("the replacement for `{}` must point to a path or to a git repository", key)));
            }

            replacements.push(Replacement {name, version, dependency});
        }

        Ok(replacements)
    }

    /// The names of the crates the `[patch]` tables mention.
    pub fn patched_names(&self) -> BTreeSet<String> {
        self.table("patch").into_iter()
            .flat_map(|sources| sources.iter())
            .filter_map(|(_, entries)| entries.as_table_like())
            .flat_map(|entries| entries.iter())
            .map(|(name, item)| {
                item.as_table_like()
                    .and_then(|table| table.get("package"))
                    .and_then(|package| package.as_str())
                    .unwrap_or(name)
                    .to_string()
            })
            .collect()
    }

    /// The crates the `[replace]` table swaps.
    pub fn replaced_crates(&self) -> BTreeSet<(String, CargoSemver)> {
        self.table("replace").into_iter()
            .flat_map(|table| table.iter())
            .filter_map(|(key, _)| parse_replace_key(key))
            .collect()
    }

    /// Whether the resolution prefers the versions supporting the
    /// project's Rust version, which Cargo does starting from the third
    /// version of its resolver (the default of the 2024 edition).
    pub fn prefers_compatible_rust_versions(&self) -> bool {
        let field = |table: Option<&dyn TableLike>, name: &str| {
            table.and_then(|table| table.get(name)).and_then(|item| item.as_str()).map(|value| value.to_string())
        };

        let resolver
            = field(self.workspace(), "resolver").or_else(|| field(self.package(), "resolver"));

        match resolver {
            Some(resolver) => resolver.parse::<u32>().is_ok_and(|resolver| resolver >= 3),
            None => field(self.package(), "edition").and_then(|edition| edition.parse::<u32>().ok()).is_some_and(|edition| edition >= 2024),
        }
    }

    pub fn rust_version(&self, workspace: Option<&Manifest>) -> Option<RustVersion> {
        self.package_field("rust-version", workspace)
            .or_else(|| self.package_field("rust_version", workspace))
            .and_then(|version| parse_rust_version(&version))
    }

    /// Looks for the manifest declaring the workspace a crate belongs to.
    /// Returns `None` when the crate is its own root.
    pub fn find_workspace_root(&self, boundary: Option<&Path>) -> Result<Option<Manifest>, Error> {
        if self.workspace().is_some() {
            return Ok(None);
        }

        let explicit
            = self.package().and_then(|package| package.get("workspace")).and_then(|item| item.as_str());

        if let Some(explicit) = explicit {
            return Ok(Some(Manifest::read(&self.dir.with_join_str(explicit))?));
        }

        let mut current
            = self.dir.clone();

        while let Some(parent) = current.dirname() {
            if parent == current || boundary.is_some_and(|boundary| !boundary.contains(&parent)) {
                break;
            }

            if Manifest::exists(&parent) {
                let candidate
                    = Manifest::read(&parent)?;

                if candidate.workspace().is_some() {
                    return Ok(Some(candidate));
                }
            }

            current = parent;
        }

        Ok(None)
    }

    /// Builds the summary of the package this manifest declares.
    pub fn summarize(&self, workspace: Option<&Manifest>, source: CrateSource, context: &ManifestContext<'_>) -> Result<Summary, Error> {
        let package
            = self.package()
                .ok_or_else(|| manifest_error(&self.dir, "this manifest doesn't declare a package"))?;

        let name
            = package.get("name").and_then(|item| item.as_str())
                .ok_or_else(|| manifest_error(&self.dir, "the package has no name"))?
                .to_string();

        let version = match self.package_field("version", workspace) {
            Some(version) => CargoSemver::parse(&version)
                .ok_or_else(|| manifest_error(&self.dir, format!("invalid version ({})", version)))?,

            None => CargoSemver::parse("0.0.0").unwrap(),
        };

        let links
            = package.get("links").and_then(|item| item.as_str()).map(|links| links.to_string());

        let mut dependencies
            = Vec::new();

        self.collect_dependencies(self.document.as_table(), None, workspace, context, &mut dependencies)?;

        if let Some(targets) = self.table("target") {
            for (target, tables) in targets.iter() {
                if let Some(tables) = tables.as_table_like() {
                    self.collect_dependencies(tables, Some(target.to_string()), workspace, context, &mut dependencies)?;
                }
            }
        }

        let mut features
            = BTreeMap::new();

        if let Some(table) = self.table("features") {
            for (feature, values) in table.iter() {
                let values = values.as_array()
                    .map(|array| array.iter().filter_map(|value| value.as_str()).map(|value| value.to_string()).collect())
                    .unwrap_or_default();

                features.insert(feature.to_string(), values);
            }
        }

        add_implicit_features(&mut features, &dependencies);

        Ok(Summary {
            name,
            version,
            source,
            dependencies,
            features,
            checksum: None,
            yanked: false,
            links,
            rust_version: self.rust_version(workspace),
            replaced_by: None,
        })
    }

    /// A string field of `[package]`, following `field.workspace = true`.
    fn package_field(&self, field: &str, workspace: Option<&Manifest>) -> Option<String> {
        let item
            = self.package()?.get(field)?;

        if let Some(value) = item.as_str() {
            return Some(value.to_string());
        }

        if !is_inherited(item) {
            return None;
        }

        workspace?.workspace()?
            .get("package")?.as_table_like()?
            .get(field)?.as_str()
            .map(|value| value.to_string())
    }

    fn collect_dependencies(&self, tables: &dyn TableLike, target: Option<String>, workspace: Option<&Manifest>, context: &ManifestContext<'_>, dependencies: &mut Vec<Dependency>) -> Result<(), Error> {
        for (table_name, kind) in DEPENDENCY_TABLES {
            let Some(table) = tables.get(table_name).and_then(|item| item.as_table_like()) else {
                continue;
            };

            for (name, item) in table.iter() {
                dependencies.push(self.parse_dependency(name, item, kind, target.clone(), workspace, context)?);
            }
        }

        Ok(())
    }

    fn parse_dependency(&self, name: &str, item: &Item, kind: DependencyKind, target: Option<String>, workspace: Option<&Manifest>, context: &ManifestContext<'_>) -> Result<Dependency, Error> {
        let mut spec
            = DependencySpec::from_item(item, &self.dir)
                .ok_or_else(|| manifest_error(&self.dir, format!("the `{}` dependency is neither a string nor a table", name)))?;

        if spec.inherited {
            let workspace
                = workspace
                    .ok_or_else(|| manifest_error(&self.dir, format!("`{}` is inherited from a workspace, but no workspace was found", name)))?;

            let inherited
                = workspace.workspace()
                    .and_then(|table| table.get("dependencies"))
                    .and_then(|item| item.as_table_like())
                    .and_then(|table| table.get(name))
                    .and_then(|item| DependencySpec::from_item(item, &workspace.dir))
                    .ok_or_else(|| manifest_error(&self.dir, format!("`{}` isn't listed in the `workspace.dependencies` of {}", name, workspace.dir.to_file_string())))?;

            spec = spec.inherit(inherited);
        }

        // Manifests name registries; published crates give their index
        let registry = match (&spec.registry, &spec.registry_index) {
            (Some(registry), _) => context.registries.by_name(registry).ok_or_else(|| manifest_error(&self.dir, format!(
                "`{}` comes from the `{}` registry, which isn't defined; declare it in the `cargoRegistries` setting or in .cargo/config.toml",
                name,
                registry,
            )))?,

            (None, Some(index)) => context.registries.from_url(index),
            (None, None) => None,
        };

        let requirement = match &spec.version {
            Some(version) => CargoRequirement::from_file_string(version)
                .map_err(|err| manifest_error(&self.dir, format!("`{}`: {}", name, err)))?,

            None => CargoRequirement::any(),
        };

        let source = match (&spec.path, &spec.git, &context.path_dependencies) {
            // Cargo ignores the paths of the dependencies of a crate that
            // doesn't come from the disk; they're crates of the same source
            (Some(_), _, PathDependencies::Git(source))
                => DependencySource::Git((*source).clone()),

            (Some(path), _, PathDependencies::Local)
                => DependencySource::Path(path.clone()),

            (None, Some(git), _)
                => DependencySource::Git(git.clone()),

            (None, None, _)
                => DependencySource::Registry(registry),
        };

        Ok(Dependency {
            name_in_toml: name.to_string(),
            package: spec.package.unwrap_or_else(|| name.to_string()),
            req: requirement.to_req(),
            requirement,
            features: spec.features,
            default_features: spec.default_features.unwrap_or(true),
            optional: spec.optional,
            kind,
            target,
            source,
        })
    }

    /// The manifest as it has to be written once the crate leaves its
    /// workspace: inherited fields are inlined, and the tables that only
    /// make sense for a workspace root are dropped. This is what
    /// `cargo package` (and `cargo vendor`) do.
    pub fn to_standalone(&self, workspace: Option<&Manifest>) -> String {
        let mut document
            = self.document.clone();

        let workspace_table = |name: &str| {
            workspace
                .and_then(|workspace| workspace.workspace())
                .and_then(|table| table.get(name))
                .and_then(|item| item.as_table_like())
        };

        if let Some(package) = document.get_mut("package").and_then(|item| item.as_table_like_mut()) {
            let inherited = package.iter()
                .filter(|(_, item)| is_inherited(item))
                .map(|(key, _)| key.to_string())
                .collect::<Vec<_>>();

            for key in inherited {
                match workspace_table("package").and_then(|table| table.get(&key)) {
                    Some(value) => {
                        package.insert(&key, detached(value));
                    },

                    None => {
                        package.remove(&key);
                    },
                }
            }

            package.remove("workspace");
        }

        let inline_dependencies = |tables: &mut dyn TableLike| {
            for (table_name, _) in DEPENDENCY_TABLES {
                let Some(table) = tables.get_mut(table_name).and_then(|item| item.as_table_like_mut()) else {
                    continue;
                };

                let inherited = table.iter()
                    .filter(|(_, item)| is_inherited(item))
                    .map(|(key, _)| key.to_string())
                    .collect::<Vec<_>>();

                for key in inherited {
                    let Some(member) = table.get(&key).and_then(|item| DependencySpec::from_item(item, &self.dir)) else {
                        continue;
                    };

                    let Some(inherited) = workspace_table("dependencies").and_then(|dependencies| dependencies.get(&key)).and_then(|item| DependencySpec::from_item(item, &self.dir)) else {
                        continue;
                    };

                    table.insert(&key, Item::Value(Value::InlineTable(member.inherit(inherited).to_inline_table())));
                }
            }
        };

        inline_dependencies(document.as_table_mut());

        if let Some(targets) = document.get_mut("target").and_then(|item| item.as_table_like_mut()) {
            for (_, tables) in targets.iter_mut() {
                if let Some(tables) = tables.as_table_like_mut() {
                    inline_dependencies(tables);
                }
            }
        }

        // Lints don't apply to dependencies, and the other tables are only
        // read from the root of a workspace
        if document.get("lints").is_some_and(is_inherited) {
            document.remove("lints");
        }

        for table in ["workspace", "patch", "replace", "profile"] {
            document.remove(table);
        }

        document.to_string()
    }
}

/// Whether an item is `{ workspace = true }` (or `item.workspace = true`).
fn is_inherited(item: &Item) -> bool {
    item.as_table_like()
        .and_then(|table| table.get("workspace"))
        .and_then(|item| item.as_bool())
        .unwrap_or(false)
}

/// A copy of a value without the whitespace and comments that surrounded
/// it in the document it was taken from.
fn detached(item: &Item) -> Item {
    match item {
        Item::Value(value) => {
            let mut value
                = value.clone();

            value.decor_mut().clear();

            Item::Value(value)
        },

        Item::Table(table) => {
            let mut inline
                = table.clone().into_inline_table();

            inline.decor_mut().clear();

            Item::Value(Value::InlineTable(inline))
        },

        _ => item.clone(),
    }
}

/// `workspace.members` entries are paths in which `*` and `?` match within
/// a path segment.
fn expand_member_pattern(root: &Path, pattern: &str) -> Vec<Path> {
    let mut candidates
        = vec![root.clone()];

    for segment in pattern.split('/').filter(|segment| !segment.is_empty() && *segment != ".") {
        let is_glob
            = segment.contains(['*', '?', '[']);

        if !is_glob {
            candidates = candidates.into_iter()
                .map(|candidate| candidate.with_join_str(segment))
                .collect();

            continue;
        }

        let Ok(matcher) = globset::Glob::new(segment).map(|glob| glob.compile_matcher()) else {
            return vec![];
        };

        candidates = candidates.into_iter()
            .flat_map(|candidate| {
                let mut matches = candidate.fs_read_dir().ok()
                    .into_iter()
                    .flatten()
                    .filter_map(|entry| entry.ok())
                    .filter(|entry| entry.file_type().is_ok_and(|file_type| file_type.is_dir()))
                    .filter_map(|entry| entry.file_name().into_string().ok())
                    .filter(|name| matcher.is_match(name))
                    .map(|name| candidate.with_join_str(name))
                    .collect::<Vec<_>>();

                matches.sort();
                matches
            })
            .collect();
    }

    candidates
}

/// A dependency as written in a manifest, before inheritance.
#[derive(Clone, Debug, Default)]
pub struct DependencySpec {
    pub version: Option<String>,
    pub package: Option<String>,
    pub path: Option<Path>,
    pub git: Option<CargoGitSource>,
    pub registry: Option<String>,
    pub registry_index: Option<String>,
    pub features: Vec<String>,
    pub default_features: Option<bool>,
    pub optional: bool,
    pub inherited: bool,
}

impl DependencySpec {
    pub fn from_item(item: &Item, base_dir: &Path) -> Option<DependencySpec> {
        if let Some(version) = item.as_str() {
            return Some(DependencySpec {
                version: Some(version.to_string()),
                ..Default::default()
            });
        }

        let table
            = item.as_table_like()?;

        let string
            = |key: &str| table.get(key).and_then(|item| item.as_str()).map(|value| value.to_string());

        let boolean
            = |key: &str| table.get(key).and_then(|item| item.as_bool());

        let git = string("git").map(|url| {
            let git_ref = if let Some(branch) = string("branch") {
                CargoGitRef::Branch(branch)
            } else if let Some(tag) = string("tag") {
                CargoGitRef::Tag(tag)
            } else if let Some(rev) = string("rev") {
                CargoGitRef::Rev(rev)
            } else {
                CargoGitRef::DefaultBranch
            };

            CargoGitSource::new(&url, &git_ref)
        });

        Some(DependencySpec {
            version: string("version"),
            package: string("package"),
            path: string("path").map(|path| base_dir.with_join_str(path)),
            git,
            registry: string("registry"),
            registry_index: string("registry-index").or_else(|| string("registry_index")),
            features: table.get("features")
                .and_then(|item| item.as_array())
                .map(|array| array.iter().filter_map(|value| value.as_str()).map(|value| value.to_string()).collect())
                .unwrap_or_default(),
            default_features: boolean("default-features").or_else(|| boolean("default_features")),
            optional: boolean("optional").unwrap_or(false),
            inherited: boolean("workspace").unwrap_or(false),
        })
    }

    /// Applies what a member may add to a dependency it inherits: more
    /// features, `optional`, and enabling back the default features.
    fn inherit(self, inherited: DependencySpec) -> DependencySpec {
        let mut features
            = inherited.features;

        for feature in self.features {
            if !features.contains(&feature) {
                features.push(feature);
            }
        }

        let default_features = match (inherited.default_features, self.default_features) {
            (Some(false), Some(true)) => Some(true),
            (inherited, _) => inherited,
        };

        DependencySpec {
            features,
            default_features,
            optional: self.optional,
            inherited: false,
            ..inherited
        }
    }

    fn to_inline_table(&self) -> InlineTable {
        let mut table
            = InlineTable::new();

        if let Some(version) = &self.version {
            table.insert("version", version.as_str().into());
        }

        if let Some(package) = &self.package {
            table.insert("package", package.as_str().into());
        }

        if let Some(registry) = &self.registry {
            table.insert("registry", registry.as_str().into());
        }

        if let Some(registry_index) = &self.registry_index {
            table.insert("registry-index", registry_index.as_str().into());
        }

        if let Some(git) = &self.git {
            table.insert("git", git.url().into());

            match git.git_ref() {
                CargoGitRef::DefaultBranch => {},
                CargoGitRef::Branch(branch) => {table.insert("branch", branch.into());},
                CargoGitRef::Tag(tag) => {table.insert("tag", tag.into());},
                CargoGitRef::Rev(rev) => {table.insert("rev", rev.into());},
            }
        }

        // The location doesn't matter once the crate left the disk (Cargo
        // looks the dependency up in the source the crate itself comes
        // from), only that there is one
        if let Some(path) = &self.path {
            table.insert("path", path.to_file_string().into());
        }

        if !self.features.is_empty() {
            table.insert("features", Value::Array(self.features.iter().map(|feature| feature.as_str()).collect()));
        }

        if let Some(default_features) = self.default_features {
            table.insert("default-features", default_features.into());
        }

        if self.optional {
            table.insert("optional", true.into());
        }

        table
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(dir: &str, text: &str) -> Manifest {
        Manifest::parse(Path::from_file_string(dir).unwrap(), text.to_string()).unwrap()
    }

    const WORKSPACE: &str = r#"
[workspace]
members = ["crates/*"]

[workspace.package]
version = "1.2.3"
edition = "2024"

[workspace.dependencies]
core = { path = "crates/core" }
serde = { version = "1.0.207", default-features = false, features = ["derive"] }
tokio = "1.39"
"#;

    const MEMBER: &str = r#"
[package]
name = "app"
version.workspace = true
edition.workspace = true
links = "app"

[dependencies]
core = { workspace = true }
serde = { workspace = true, features = ["rc"], optional = true }
tokio.workspace = true
renamed = { package = "real-name", version = ">= 1.2, < 2" }
upstream = { git = "https://example.com/upstream.git", branch = "next" }
gated = { version = "0.4", optional = true }

[target.'cfg(unix)'.dependencies]
libc = "0.2"

[build-dependencies]
cc = "1"

[dev-dependencies]
rstest = "0.26"

[features]
default = ["std"]
std = ["serde?/std"]
full = ["dep:gated", "serde"]

[lints]
workspace = true
"#;

    #[test]
    fn test_summary_with_inheritance() {
        let workspace
            = manifest("/repo", WORKSPACE);
        let member
            = manifest("/repo/crates/app", MEMBER);

        let summary
            = member.summarize(Some(&workspace), CrateSource::Path(member.dir.clone()), &ManifestContext::local(&Registries::default())).unwrap();

        assert_eq!(summary.name, "app");
        assert_eq!(summary.version.to_string(), "1.2.3");
        assert_eq!(summary.links.as_deref(), Some("app"));

        let dependency
            = |name: &str| summary.dependencies.iter().find(|dependency| dependency.name_in_toml == name).unwrap();

        assert_eq!(dependency("core").source, DependencySource::Path(Path::from_file_string("/repo/crates/core").unwrap()));

        assert_eq!(dependency("serde").requirement.as_str(), "1.0.207");
        assert_eq!(dependency("serde").features, vec!["derive", "rc"]);
        assert!(!dependency("serde").default_features);
        assert!(dependency("serde").optional);

        assert_eq!(dependency("tokio").requirement.as_str(), "1.39");
        assert!(dependency("tokio").default_features);

        assert_eq!(dependency("renamed").package, "real-name");
        assert_eq!(dependency("renamed").requirement.as_str(), ">=1.2,<2");

        assert_eq!(dependency("upstream").source, DependencySource::Git(CargoGitSource::from_file_string("https://example.com/upstream.git?branch=next").unwrap()));

        assert_eq!(dependency("libc").target.as_deref(), Some("cfg(unix)"));
        assert_eq!(dependency("cc").kind, DependencyKind::Build);
        assert_eq!(dependency("rstest").kind, DependencyKind::Development);

        // `serde` gets an implicit feature, `gated` doesn't (`dep:gated`)
        assert_eq!(summary.features.get("serde"), Some(&vec!["dep:serde".to_string()]));
        assert!(!summary.features.contains_key("gated"));
    }

    #[test]
    fn test_path_dependencies_of_git_crates_stay_in_their_repository() {
        let source
            = CargoGitSource::from_file_string("https://example.com/repo.git").unwrap();

        let workspace
            = manifest("/checkout", WORKSPACE);
        let member
            = manifest("/checkout/crates/app", MEMBER);

        let summary
            = member.summarize(Some(&workspace), CrateSource::Git {source: source.clone(), commit: "0".repeat(40)}, &ManifestContext::git(&source, &Registries::default())).unwrap();

        let core
            = summary.dependencies.iter().find(|dependency| dependency.name_in_toml == "core").unwrap();

        assert_eq!(core.source, DependencySource::Git(source));
    }

    fn registries() -> Registries {
        Registries {
            default_index: "https://index.crates.io/".to_string(),
            names: BTreeMap::from([("internal".to_string(), "https://crates.example.org/index/".to_string())]),
            tokens: BTreeMap::new(),
        }
    }

    #[test]
    fn test_registries() {
        let registries
            = registries();

        let app = manifest("/repo", r#"
[package]
name = "app"
rust-version = "1.70"

[dependencies]
private = { version = "1", registry = "internal" }
published = { version = "1", registry-index = "sparse+https://crates.example.org/index" }
public = { version = "1", registry-index = "https://github.com/rust-lang/crates.io-index" }
"#);

        let summary
            = app.summarize(None, CrateSource::Path(app.dir.clone()), &ManifestContext::local(&registries)).unwrap();

        let sources
            = summary.dependencies.iter().map(|dependency| (dependency.name_in_toml.as_str(), dependency.source.clone())).collect::<BTreeMap<_, _>>();

        let internal
            = DependencySource::Registry(Some("https://crates.example.org/index/".to_string()));

        assert_eq!(sources["private"], internal);
        assert_eq!(sources["published"], internal);
        assert_eq!(sources["public"], DependencySource::Registry(None));
        assert_eq!(summary.rust_version, Some((1, 70, 0)));

        let unknown
            = manifest("/repo", "[package]\nname = \"app\"\n\n[dependencies]\nprivate = { version = \"1\", registry = \"nope\" }\n");

        let error
            = unknown.summarize(None, CrateSource::Path(unknown.dir.clone()), &ManifestContext::local(&registries)).unwrap_err();

        assert!(error.to_string().contains("which isn't defined"), "{}", error);
    }

    #[test]
    fn test_patches_and_replacements() {
        let registries
            = registries();

        let root = manifest("/repo", r#"
[workspace]
resolver = "3"

[patch.crates-io]
serde = { path = "forks/serde" }
renamed = { package = "real", git = "https://example.com/real.git", branch = "fix" }

[patch.internal]
private = { path = "forks/private" }

[patch."https://github.com/Example/Upstream.git"]
upstream = { path = "forks/upstream" }

[replace]
"log:0.4.20" = { path = "forks/log" }
"https://github.com/rust-lang/crates.io-index#libc@0.2.150" = { git = "https://example.com/libc.git" }
"#);

        let context
            = ManifestContext::local(&registries);

        let patches = root.patches(&context).unwrap().into_iter()
            .map(|(target, dependency)| (dependency.package.clone(), (target, dependency.source)))
            .collect::<BTreeMap<_, _>>();

        assert_eq!(patches["serde"], (PatchTarget::Registry(None), DependencySource::Path(Path::from_file_string("/repo/forks/serde").unwrap())));
        assert_eq!(patches["real"].0, PatchTarget::Registry(None));
        assert_eq!(patches["private"].0, PatchTarget::Registry(Some("https://crates.example.org/index/".to_string())));
        assert_eq!(patches["upstream"].0, PatchTarget::Git("https://github.com/example/upstream".to_string()));

        assert_eq!(root.patched_names(), BTreeSet::from(["private".to_string(), "real".to_string(), "serde".to_string(), "upstream".to_string()]));

        let replacements = root.replacements(&context).unwrap().into_iter()
            .map(|replacement| format!("{} {}", replacement.name, replacement.version))
            .collect::<Vec<_>>();

        assert_eq!(replacements, vec!["log 0.4.20", "libc 0.2.150"]);
        assert_eq!(root.replaced_crates().len(), 2);

        assert!(root.prefers_compatible_rust_versions());
        assert!(!manifest("/repo", "[workspace]\nresolver = \"2\"\n").prefers_compatible_rust_versions());
        assert!(manifest("/repo", "[package]\nname = \"app\"\nedition = \"2024\"\n").prefers_compatible_rust_versions());
        assert!(!manifest("/repo", "[package]\nname = \"app\"\nedition = \"2021\"\n").prefers_compatible_rust_versions());

        let invalid
            = manifest("/repo", "[patch.crates-io]\nserde = \"1\"\n");

        assert!(invalid.patches(&context).is_err());
    }

    #[test]
    fn test_standalone_manifest() {
        let workspace
            = manifest("/repo", WORKSPACE);
        let member
            = manifest("/repo/crates/app", MEMBER);

        let standalone
            = manifest("/vendor/app", &member.to_standalone(Some(&workspace)));

        // The standalone manifest must describe the very same package
        // without any workspace around
        let summary
            = standalone.summarize(None, CrateSource::Path(standalone.dir.clone()), &ManifestContext::local(&Registries::default())).unwrap();

        assert_eq!(summary.version.to_string(), "1.2.3");
        assert!(standalone.document.get("lints").is_none());

        let serde
            = summary.dependencies.iter().find(|dependency| dependency.name_in_toml == "serde").unwrap();

        assert_eq!(serde.requirement.as_str(), "1.0.207");
        assert_eq!(serde.features, vec!["derive", "rc"]);
        assert!(serde.optional);
        assert!(!serde.default_features);

        let core
            = summary.dependencies.iter().find(|dependency| dependency.name_in_toml == "core").unwrap();

        assert!(matches!(core.source, DependencySource::Path(_)));
    }

    #[test]
    fn test_workspace_members() {
        let root
            = Path::temp_dir().unwrap();

        for (path, content) in [
            ("Cargo.toml", "[workspace]\nmembers = [\"crates/*\", \"tools/cli\"]\nexclude = [\"crates/skipped\"]\n"),
            ("crates/a/Cargo.toml", "[package]\nname = \"a\"\n"),
            ("crates/b/Cargo.toml", "[package]\nname = \"b\"\n"),
            ("crates/skipped/Cargo.toml", "[package]\nname = \"skipped\"\n"),
            ("crates/not-a-crate/README.md", ""),
            ("tools/cli/Cargo.toml", "[package]\nname = \"cli\"\n"),
        ] {
            root.with_join_str(path).fs_create_parent().unwrap().fs_write_text(content).unwrap();
        }

        let members = Manifest::read(&root).unwrap().member_dirs().unwrap().iter()
            .map(|dir| dir.relative_to(&root).to_file_string())
            .collect::<Vec<_>>();

        assert_eq!(members, vec!["crates/a", "crates/b", "tools/cli"]);

        let member
            = Manifest::read(&root.with_join_str("crates/a")).unwrap();

        assert_eq!(member.find_workspace_root(None).unwrap().unwrap().dir, root);
    }
}
