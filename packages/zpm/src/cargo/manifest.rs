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

use super::{CrateSource, Dependency, DependencyKind, DependencySource, Summary, add_implicit_features};

pub const MANIFEST_NAME: &str
    = "Cargo.toml";

const DEPENDENCY_TABLES: [(&str, DependencyKind); 5] = [
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

    /// The tables changing where crates come from, which only apply when
    /// set at the root of a workspace.
    pub fn source_overrides(&self) -> Vec<&'static str> {
        ["patch", "replace"].into_iter()
            .filter(|name| self.table(name).is_some_and(|table| !table.is_empty()))
            .collect()
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
    pub fn summarize(&self, workspace: Option<&Manifest>, source: CrateSource, path_dependencies: &PathDependencies<'_>) -> Result<Summary, Error> {
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

        self.collect_dependencies(self.document.as_table(), None, workspace, path_dependencies, &mut dependencies)?;

        if let Some(targets) = self.table("target") {
            for (target, tables) in targets.iter() {
                if let Some(tables) = tables.as_table_like() {
                    self.collect_dependencies(tables, Some(target.to_string()), workspace, path_dependencies, &mut dependencies)?;
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

    fn collect_dependencies(&self, tables: &dyn TableLike, target: Option<String>, workspace: Option<&Manifest>, path_dependencies: &PathDependencies<'_>, dependencies: &mut Vec<Dependency>) -> Result<(), Error> {
        for (table_name, kind) in DEPENDENCY_TABLES {
            let Some(table) = tables.get(table_name).and_then(|item| item.as_table_like()) else {
                continue;
            };

            for (name, item) in table.iter() {
                dependencies.push(self.parse_dependency(name, item, kind, target.clone(), workspace, path_dependencies)?);
            }
        }

        Ok(())
    }

    fn parse_dependency(&self, name: &str, item: &Item, kind: DependencyKind, target: Option<String>, workspace: Option<&Manifest>, path_dependencies: &PathDependencies<'_>) -> Result<Dependency, Error> {
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

        // Silently resolving those from the default registry would be wrong
        if let Some(registry) = &spec.registry {
            return Err(manifest_error(&self.dir, format!("`{}` comes from the `{}` registry; alternate registries aren't supported yet", name, registry)));
        }

        let requirement = match &spec.version {
            Some(version) => CargoRequirement::from_file_string(version)
                .map_err(|err| manifest_error(&self.dir, format!("`{}`: {}", name, err)))?,

            None => CargoRequirement::any(),
        };

        let source = match (&spec.path, &spec.git, path_dependencies) {
            // Cargo ignores the paths of the dependencies of a crate that
            // doesn't come from the disk; they're crates of the same source
            (Some(_), _, PathDependencies::Git(source))
                => DependencySource::Git((*source).clone()),

            (Some(path), _, PathDependencies::Local)
                => DependencySource::Path(path.clone()),

            (None, Some(git), _)
                => DependencySource::Git(git.clone()),

            (None, None, _)
                => DependencySource::Registry,
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
struct DependencySpec {
    version: Option<String>,
    package: Option<String>,
    path: Option<Path>,
    git: Option<CargoGitSource>,
    registry: Option<String>,
    features: Vec<String>,
    default_features: Option<bool>,
    optional: bool,
    inherited: bool,
}

impl DependencySpec {
    fn from_item(item: &Item, base_dir: &Path) -> Option<DependencySpec> {
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
            = member.summarize(Some(&workspace), CrateSource::Path(member.dir.clone()), &PathDependencies::Local).unwrap();

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
            = member.summarize(Some(&workspace), CrateSource::Git {source: source.clone(), commit: "0".repeat(40)}, &PathDependencies::Git(&source)).unwrap();

        let core
            = summary.dependencies.iter().find(|dependency| dependency.name_in_toml == "core").unwrap();

        assert_eq!(core.source, DependencySource::Git(source));
    }

    #[test]
    fn test_unsupported_sources_are_reported() {
        let alternate
            = manifest("/repo", "[package]\nname = \"app\"\n\n[dependencies]\nprivate = { version = \"1\", registry = \"internal\" }\n");

        let error
            = alternate.summarize(None, CrateSource::Path(alternate.dir.clone()), &PathDependencies::Local).unwrap_err();

        assert!(error.to_string().contains("alternate registries aren't supported yet"), "{}", error);

        let patched
            = manifest("/repo", "[workspace]\n\n[patch.crates-io]\nserde = { path = \"../serde\" }\n");

        assert_eq!(patched.source_overrides(), vec!["patch"]);
        assert!(manifest("/repo", WORKSPACE).source_overrides().is_empty());
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
            = standalone.summarize(None, CrateSource::Path(standalone.dir.clone()), &PathDependencies::Local).unwrap();

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
