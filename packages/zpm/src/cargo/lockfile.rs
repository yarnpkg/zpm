//! `Cargo.lock` reader and writer.
//!
//! Yarn's lockfile is the source of truth for the islands using the cargo
//! linker; the `Cargo.lock` Cargo requires is derived from it at link
//! time. It's also read the first time an island gets resolved, so that
//! adopting Yarn doesn't upgrade anything.

use std::collections::{BTreeMap, BTreeSet};

use toml_edit::DocumentMut;
use zpm_primitives::{CargoGitRef, CargoGitSource, CargoSemver};

use super::{RegistryIndex, RustVersion, registry_source_id};

pub const LOCKFILE_NAME: &str
    = "Cargo.lock";

/// The format Cargo has been writing since 1.78.
pub const DEFAULT_LOCKFILE_VERSION: u32
    = 4;

/// The format new lockfiles are written in. Like Cargo, the newest one
/// the workspace's oldest supported Rust release can read.
pub fn lockfile_version_for(rust_version: Option<RustVersion>) -> u32 {
    match rust_version {
        Some(rust_version) if rust_version < (1, 78, 0) => 3,
        _ => DEFAULT_LOCKFILE_VERSION,
    }
}

/// What a `Cargo.lock` pins.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LockedPackages {
    /// Version of the lockfile format
    pub version: Option<u32>,

    /// The versions of every package, whatever its source
    pub versions: BTreeMap<String, BTreeSet<CargoSemver>>,

    /// The commit each git source is pinned to
    pub git: BTreeMap<CargoGitSource, String>,
}

fn parse_git_source(source: &str) -> Option<(CargoGitSource, String)> {
    let (source, commit)
        = source.strip_prefix("git+")?.split_once('#')?;

    let (url, query)
        = source.split_once('?').unwrap_or((source, ""));

    // Older lockfiles don't percent-encode the ref; going through the
    // parsed form gives both the same spelling
    let raw: CargoGitSource
        = format!("{}{}{}", url, if query.is_empty() {""} else {"?"}, query).parse().ok()?;

    Some((CargoGitSource::new(url, &raw.git_ref()), commit.to_string()))
}

pub fn parse_lockfile(text: &str) -> Option<LockedPackages> {
    let document
        = text.parse::<DocumentMut>().ok()?;

    let mut locked = LockedPackages {
        version: document.get("version").and_then(|item| item.as_integer()).map(|version| version as u32),
        ..Default::default()
    };

    let packages
        = document.get("package").and_then(|item| item.as_array_of_tables());

    for package in packages.into_iter().flatten() {
        let (Some(name), Some(version)) = (package.get("name").and_then(|item| item.as_str()), package.get("version").and_then(|item| item.as_str())) else {
            continue;
        };

        if let Some(version) = CargoSemver::parse(version) {
            locked.versions.entry(name.to_string()).or_default().insert(version);
        }

        let git_source
            = package.get("source").and_then(|item| item.as_str()).and_then(parse_git_source);

        if let Some((source, commit)) = git_source {
            locked.git.insert(source, commit);
        }
    }

    Some(locked)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LockSource {
    /// Workspace members and path dependencies have no source
    Path,
    Registry(RegistryIndex),
    Git {
        source: CargoGitSource,
        commit: String,
    },
}

impl LockSource {
    fn render(&self, lockfile_version: u32) -> Option<String> {
        match self {
            LockSource::Path => None,
            LockSource::Registry(registry) => Some(registry_source_id(registry)),

            LockSource::Git {source, commit} => {
                // The ref only got percent-encoded with the fourth version
                let query = match (source.git_ref(), lockfile_version >= 4) {
                    (CargoGitRef::DefaultBranch, _) => String::new(),
                    (_, true) => source.as_str().split_once('?').map(|(_, query)| format!("?{}", query)).unwrap_or_default(),
                    (CargoGitRef::Branch(branch), false) => format!("?branch={}", branch),
                    (CargoGitRef::Tag(tag), false) => format!("?tag={}", tag),
                    (CargoGitRef::Rev(rev), false) => format!("?rev={}", rev),
                };

                Some(format!("git+{}{}#{}", source.url(), query, commit))
            },
        }
    }
}

#[derive(Clone, Debug)]
pub struct LockPackage {
    pub name: String,
    pub version: CargoSemver,
    pub source: LockSource,

    /// Hex sha256 of the `.crate` file; registry crates only
    pub checksum: Option<String>,

    /// Indices of the dependencies in the list of packages
    pub dependencies: Vec<usize>,

    /// Index of the package a `[replace]` table swaps this one for
    pub replace: Option<usize>,

    /// Whether the package is a patch nothing uses; those are listed
    /// apart, without their dependencies
    pub is_unused_patch: bool,
}

/// Renders a `Cargo.lock` the way Cargo does, so that `cargo build
/// --locked` accepts it as-is and an unchanged resolution yields an
/// unchanged file.
pub fn render_lockfile(packages: &[LockPackage], lockfile_version: u32) -> String {
    let sources = packages.iter()
        .map(|package| package.source.render(lockfile_version))
        .collect::<Vec<_>>();

    let sort_key
        = |index: &usize| (packages[*index].name.as_str(), &packages[*index].version, sources[*index].as_deref());

    let mut order
        = (0..packages.len()).collect::<Vec<_>>();

    order.sort_by(|a, b| sort_key(a).cmp(&sort_key(b)));

    let mut by_name: BTreeMap<&str, usize>
        = BTreeMap::new();
    let mut by_name_and_version: BTreeMap<(&str, &CargoSemver), usize>
        = BTreeMap::new();

    // Unused patches aren't part of the graph, so they don't make the
    // names of the packages that are any more ambiguous
    for package in packages.iter().filter(|package| !package.is_unused_patch) {
        *by_name.entry(package.name.as_str()).or_default() += 1;
        *by_name_and_version.entry((package.name.as_str(), &package.version)).or_default() += 1;
    }

    // Dependencies are only qualified as much as needed to tell them apart
    let dependency_entry = |index: usize| {
        let package
            = &packages[index];

        if by_name[package.name.as_str()] == 1 {
            return package.name.clone();
        }

        if by_name_and_version[&(package.name.as_str(), &package.version)] == 1 {
            return format!("{} {}", package.name, package.version);
        }

        match &sources[index] {
            Some(source) => format!("{} {} ({})", package.name, package.version, source.split('#').next().unwrap()),
            None => format!("{} {}", package.name, package.version),
        }
    };

    let mut output
        = String::new();

    output.push_str("# This file is automatically @generated by Cargo.\n");
    output.push_str("# It is not intended for manual editing.\n");
    output.push_str(&format!("version = {}\n", lockfile_version));

    for index in order.iter().copied().filter(|index| !packages[*index].is_unused_patch) {
        let package
            = &packages[index];

        output.push_str("\n[[package]]\n");
        output.push_str(&format!("name = \"{}\"\n", package.name));
        output.push_str(&format!("version = \"{}\"\n", package.version));

        if let Some(source) = &sources[index] {
            output.push_str(&format!("source = \"{}\"\n", source));
        }

        if let Some(checksum) = &package.checksum {
            output.push_str(&format!("checksum = \"{}\"\n", checksum));
        }

        let mut dependencies = package.dependencies.iter().copied()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();

        dependencies.sort_by(|a, b| sort_key(a).cmp(&sort_key(b)));

        if !dependencies.is_empty() {
            output.push_str("dependencies = [\n");

            for dependency in dependencies {
                output.push_str(&format!(" \"{}\",\n", dependency_entry(dependency)));
            }

            output.push_str("]\n");
        }

        if let Some(replace) = package.replace {
            output.push_str(&format!("replace = \"{}\"\n", dependency_entry(replace)));
        }
    }

    for index in order.iter().copied().filter(|index| packages[*index].is_unused_patch) {
        let package
            = &packages[index];

        output.push_str("\n[[patch.unused]]\n");
        output.push_str(&format!("name = \"{}\"\n", package.name));
        output.push_str(&format!("version = \"{}\"\n", package.version));

        if let Some(source) = &sources[index] {
            output.push_str(&format!("source = \"{}\"\n", source));
        }
    }

    output
}

#[cfg(test)]
mod tests {
    use zpm_utils::FromFileString;

    use super::*;

    const LOCKFILE: &str = r#"# This file is automatically @generated by Cargo.
# It is not intended for manual editing.
version = 4

[[package]]
name = "app"
version = "0.1.0"
dependencies = [
 "dialoguer",
 "thiserror 1.0.69",
 "thiserror 2.0.18",
 "wasi",
]

[[package]]
name = "dialoguer"
version = "0.11.0"
source = "git+https://github.com/arcanis/dialoguer.git?branch=mael%2Fconfirm-prompt-height#2646dce8d26caa6e7d4eee80836dffd7813afcd1"
dependencies = [
 "thiserror 1.0.69",
]

[[package]]
name = "thiserror"
version = "1.0.69"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "b6aaf5339b578ea85b50e080feb250a3e8ae8cfcdff9a461c9ec2904bc923f52"

[[package]]
name = "thiserror"
version = "2.0.18"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "4288b5bcbc7920c07a1149a35cf9590a2aa808e0bc1eafaade0b80947865fbc4"

[[package]]
name = "wasi"
version = "0.11.1+wasi-snapshot-preview1"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "ccf3ec651a847eb01de73ccad15eb7d99f80485de043efb2f370cd654f4ea44b"
"#;

    fn version(src: &str) -> CargoSemver {
        CargoSemver::parse(src).unwrap()
    }

    #[test]
    fn test_reading() {
        let locked
            = parse_lockfile(LOCKFILE).unwrap();

        assert_eq!(locked.version, Some(4));
        assert_eq!(locked.versions["thiserror"], BTreeSet::from([version("1.0.69"), version("2.0.18")]));
        assert_eq!(locked.versions["wasi"], BTreeSet::from([version("0.11.1+wasi-snapshot-preview1")]));
        assert_eq!(locked.versions["dialoguer"], BTreeSet::from([version("0.11.0")]));
        assert_eq!(locked.versions["app"], BTreeSet::from([version("0.1.0")]));

        let source
            = CargoGitSource::from_file_string("https://github.com/arcanis/dialoguer.git?branch=mael%2Fconfirm-prompt-height").unwrap();

        assert_eq!(locked.git[&source], "2646dce8d26caa6e7d4eee80836dffd7813afcd1");

        // The third version of the format wrote refs as-is
        let legacy
            = parse_lockfile(&LOCKFILE.replace("version = 4", "version = 3").replace("mael%2Fconfirm", "mael/confirm")).unwrap();

        assert_eq!(legacy.git, locked.git);
    }

    #[test]
    fn test_writing_matches_cargo() {
        let git = LockSource::Git {
            source: CargoGitSource::from_file_string("https://github.com/arcanis/dialoguer.git?branch=mael%2Fconfirm-prompt-height").unwrap(),
            commit: "2646dce8d26caa6e7d4eee80836dffd7813afcd1".to_string(),
        };

        // Listed out of order, and with duplicated edges, on purpose
        let packages = vec![
            LockPackage {name: "wasi".to_string(), version: version("0.11.1+wasi-snapshot-preview1"), source: LockSource::Registry(None), checksum: Some("ccf3ec651a847eb01de73ccad15eb7d99f80485de043efb2f370cd654f4ea44b".to_string()), dependencies: vec![], replace: None, is_unused_patch: false},
            LockPackage {name: "thiserror".to_string(), version: version("2.0.18"), source: LockSource::Registry(None), checksum: Some("4288b5bcbc7920c07a1149a35cf9590a2aa808e0bc1eafaade0b80947865fbc4".to_string()), dependencies: vec![], replace: None, is_unused_patch: false},
            LockPackage {name: "app".to_string(), version: version("0.1.0"), source: LockSource::Path, checksum: None, dependencies: vec![0, 1, 3, 4, 4], replace: None, is_unused_patch: false},
            LockPackage {name: "thiserror".to_string(), version: version("1.0.69"), source: LockSource::Registry(None), checksum: Some("b6aaf5339b578ea85b50e080feb250a3e8ae8cfcdff9a461c9ec2904bc923f52".to_string()), dependencies: vec![], replace: None, is_unused_patch: false},
            LockPackage {name: "dialoguer".to_string(), version: version("0.11.0"), source: git, checksum: None, dependencies: vec![3], replace: None, is_unused_patch: false},
        ];

        assert_eq!(render_lockfile(&packages, 4), LOCKFILE);
    }
}
