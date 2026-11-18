//! The `cargo` linker.
//!
//! Lays the crates of an island out so that Cargo builds from them without
//! touching the network, which takes three things next to the island's
//! `Cargo.toml`:
//!
//! - `.cargo/vendor`, a directory source (what `cargo vendor` produces)
//!   holding the unpacked crates;
//! - `.cargo/config.toml`, which replaces crates.io and the git
//!   repositories the island depends on with that directory;
//! - `Cargo.lock`, derived from Yarn's lockfile, so that Cargo uses the
//!   very versions Yarn resolved.
//!
//! None of this needs Cargo; it only gets involved once something is built.

use std::collections::{BTreeMap, BTreeSet};

use rayon::prelude::*;
use sha2::Digest;
use zpm_formats::iter_ext::IterExt;
use zpm_primitives::{CargoGitRef, CargoGitSource, CargoSemver, Locator, Reference};
use zpm_utils::{Path, ToFileString};

use crate::{
    build::BuildRequests,
    cargo::{
        CrateSource,
        island::island_state,
        lockfile::{DEFAULT_LOCKFILE_VERSION, LOCKFILE_NAME, LockPackage, LockSource, parse_lockfile, render_lockfile},
        manifest::{MANIFEST_NAME, Manifest, PathDependencies},
    },
    error::Error,
    fetchers::PackageData,
    install::Install,
    island::ResolvedIsland,
    linker::LinkResult,
    project::Project,
};

const CONFIG_PATH: &str
    = ".cargo/config.toml";

const VENDOR_PATH: &str
    = ".cargo/vendor";

/// The name of the directory source in Cargo's configuration.
const VENDOR_SOURCE: &str
    = "yarn-vendor";

/// Tells which crate a vendored folder was made from.
const CRATE_MARKER: &str
    = ".yarn-crate";

/// Holds what the vendor directory was generated from; see `island_state`.
const STATE_MARKER: &str
    = ".yarn-state";

const BLOCK_START: &str
    = "# >>> yarn cargo island >>>";

const BLOCK_END: &str
    = "# <<< yarn cargo island <<<";

pub fn vendor_path(workspace_path: &Path) -> Path {
    workspace_path.with_join_str(VENDOR_PATH)
}

/// Whether the artifacts of the cargo linker are where the last install
/// left them, and were generated from the manifests as they are now.
pub fn is_workspace_linked(workspace_path: &Path, state: &str) -> bool {
    workspace_path.with_join_str(LOCKFILE_NAME).fs_exists()
        && workspace_path.with_join_str(CONFIG_PATH).fs_exists()
        && vendor_path(workspace_path).with_join_str(STATE_MARKER).fs_read_text().ok().as_deref() == Some(state)
}

enum CrateData {
    /// A `.crate` file from the cache
    Archive(Path),

    /// A folder of a git checkout
    Checkout(Path),
}

struct LinkedCrate {
    locator: Locator,
    name: String,
    version: CargoSemver,
    source: LockSource,

    /// Hex sha256 of the `.crate` file; registry crates only
    checksum: Option<String>,

    /// What gets vendored; crates living in the project are used in place
    data: Option<CrateData>,

    /// Absolute path of the crates living in the project
    local_dir: Option<Path>,

    dependencies: Vec<usize>,
}

impl LinkedCrate {
    fn marker(&self) -> String {
        match (&self.source, &self.checksum) {
            (LockSource::Git {source, commit}, _) => format!("git+{}#{}\n", source.as_str(), commit),
            (_, Some(checksum)) => format!("sha256:{}\n", checksum),
            _ => String::new(),
        }
    }
}

/// The version of a crate found on the disk, read from its manifest.
fn read_crate_version(dir: &Path, source: CrateSource, git_source: Option<&CargoGitSource>) -> Result<CargoSemver, Error> {
    let manifest
        = Manifest::read(dir)?;

    let workspace
        = manifest.find_workspace_root(None)?;

    let path_dependencies = match git_source {
        Some(source) => PathDependencies::Git(source),
        None => PathDependencies::Local,
    };

    Ok(manifest.summarize(workspace.as_ref(), source, &path_dependencies)?.version)
}

fn describe_crate(project: &Project, install: &Install, locator: &Locator) -> Result<LinkedCrate, Error> {
    let package_data
        = install.package_data.get(locator);

    let name
        = locator.ident.as_str().to_string();

    match &locator.reference {
        Reference::CargoRegistry(params) => {
            let Some(PackageData::Zip {archive_path, ..}) = package_data else {
                return Err(Error::CargoError(format!("The crate {} wasn't fetched", locator.to_file_string())));
            };

            // Cargo's lockfile lists the checksum the registry advertises,
            // which the fetcher checked the archive against
            let checksum
                = hex::encode(sha2::Sha256::digest(archive_path.fs_read()?));

            Ok(LinkedCrate {
                locator: locator.clone(),
                name,
                version: params.version.to_semver(),
                source: LockSource::Registry,
                checksum: Some(checksum),
                data: Some(CrateData::Archive(archive_path.clone())),
                local_dir: None,
                dependencies: vec![],
            })
        },

        Reference::CargoGit(params) => {
            let Some(PackageData::Local {package_directory, ..}) = package_data else {
                return Err(Error::CargoError(format!("The crate {} wasn't fetched", locator.to_file_string())));
            };

            let source = CrateSource::Git {
                source: params.source.clone(),
                commit: params.commit.clone(),
            };

            Ok(LinkedCrate {
                locator: locator.clone(),
                name,
                version: read_crate_version(package_directory, source, Some(&params.source))?,
                source: LockSource::Git {
                    source: params.source.clone(),
                    commit: params.commit.clone(),
                },
                checksum: None,
                data: Some(CrateData::Checkout(package_directory.clone())),
                local_dir: None,
                dependencies: vec![],
            })
        },

        Reference::CargoPath(params) => {
            let dir
                = project.project_cwd.with_join_str(&params.path);

            Ok(LinkedCrate {
                locator: locator.clone(),
                name,
                version: read_crate_version(&dir, CrateSource::Path(dir.clone()), None)?,
                source: LockSource::Path,
                checksum: None,
                data: None,
                local_dir: Some(dir),
                dependencies: vec![],
            })
        },

        _ => Err(Error::CargoError(format!("{} can't be installed by the cargo linker", locator.to_file_string()))),
    }
}

fn unpack_archive(archive_path: &Path, destination: &Path) -> Result<(), Error> {
    let archive
        = archive_path.fs_read()?;

    let tar
        = zpm_formats::tar::unpack_tgz(&archive)?;

    let entries
        = zpm_formats::tar_iter::TarIterator::new(&tar).collect::<Result<Vec<_>, _>>()?;

    // Crates are packed in a `<name>-<version>` folder
    let entries = entries.into_iter()
        .strip_first_segment()
        .collect::<Vec<_>>();

    zpm_formats::entries_to_disk(&entries, destination)?;

    Ok(())
}

fn copy_tree(source: &Path, destination: &Path, is_root: bool) -> Result<(), Error> {
    destination.fs_create_dir_all()?;

    for entry in source.fs_read_dir()?.flatten() {
        let name
            = entry.file_name().to_string_lossy().to_string();

        if is_root && matches!(name.as_str(), ".git" | "target" | ".yarn-checkout") {
            continue;
        }

        let file_type
            = entry.file_type()?;

        let from
            = source.with_join_str(&name);
        let to
            = destination.with_join_str(&name);

        if file_type.is_dir() {
            copy_tree(&from, &to, false)?;
        } else if file_type.is_file() {
            from.fs_copy_file(&to)?;
        }
    }

    Ok(())
}

/// Git crates are copied from their checkout, with a manifest that doesn't
/// rely on the workspace they were part of.
fn copy_checkout(crate_dir: &Path, destination: &Path) -> Result<(), Error> {
    let manifest
        = Manifest::read(crate_dir)?;

    let workspace
        = manifest.find_workspace_root(None)?;

    copy_tree(crate_dir, destination, true)?;

    destination.with_join_str(MANIFEST_NAME)
        .fs_write_text(manifest.to_standalone(workspace.as_ref()))?;

    Ok(())
}

/// Puts a crate in the vendor directory, unless it's already there.
fn vendor_crate(linked: &LinkedCrate, destination: &Path) -> Result<(), Error> {
    let Some(data) = &linked.data else {
        return Ok(());
    };

    let marker
        = linked.marker();

    if destination.with_join_str(CRATE_MARKER).fs_read_text().ok().as_deref() == Some(marker.as_str()) {
        return Ok(());
    }

    // The crate only appears under its final name once complete, so an
    // interrupted install never leaves a truncated crate behind
    let staging = destination.dirname().unwrap()
        .with_join_str(format!(".staging-{}", destination.basename().unwrap()));

    if staging.fs_symlink_metadata().is_ok() {
        staging.fs_rm()?;
    }

    match data {
        CrateData::Archive(archive_path) => unpack_archive(archive_path, &staging)?,
        CrateData::Checkout(crate_dir) => copy_checkout(crate_dir, &staging)?,
    }

    // Directory sources check the files of a crate against this file. Cargo
    // accepts an empty list, and the archive was verified when fetched.
    let checksum = serde_json::json!({
        "files": {},
        "package": linked.checksum,
    });

    staging.with_join_str(".cargo-checksum.json").fs_write_text(checksum.to_string())?;
    staging.with_join_str(CRATE_MARKER).fs_write_text(&marker)?;

    if destination.fs_symlink_metadata().is_ok() {
        destination.fs_rm()?;
    }

    staging.fs_rename(destination)?;

    Ok(())
}

fn toml_string(value: &str) -> String {
    serde_json::to_string(value).unwrap()
}

/// The configuration making Cargo read crates from the vendor directory
/// rather than from where they were published.
fn render_config_block(git_sources: &BTreeSet<CargoGitSource>) -> String {
    let mut lines = vec![
        BLOCK_START.to_string(),
        "# Managed by Yarn, which rewrites this block on every install: Cargo builds".to_string(),
        "# from the crates Yarn installed rather than fetching them itself.".to_string(),
        "[source.crates-io]".to_string(),
        format!("replace-with = {}", toml_string(VENDOR_SOURCE)),
    ];

    for source in git_sources {
        lines.push(String::new());
        lines.push(format!("[source.{}]", toml_string(&format!("git+{}", source.as_str()))));
        lines.push(format!("git = {}", toml_string(source.url())));

        match source.git_ref() {
            CargoGitRef::DefaultBranch => {},
            CargoGitRef::Branch(branch) => lines.push(format!("branch = {}", toml_string(&branch))),
            CargoGitRef::Tag(tag) => lines.push(format!("tag = {}", toml_string(&tag))),
            CargoGitRef::Rev(rev) => lines.push(format!("rev = {}", toml_string(&rev))),
        }

        lines.push(format!("replace-with = {}", toml_string(VENDOR_SOURCE)));
    }

    lines.push(String::new());
    lines.push(format!("[source.{}]", VENDOR_SOURCE));
    lines.push(format!("directory = {}", toml_string(VENDOR_PATH)));
    lines.push(BLOCK_END.to_string());

    format!("{}\n", lines.join("\n"))
}

/// Inserts the managed block in a configuration file, leaving whatever
/// else the file holds untouched.
fn merge_config(existing: &str, block: &str) -> Result<String, Error> {
    let (before, after) = match (existing.find(BLOCK_START), existing.find(BLOCK_END)) {
        (Some(start), Some(end)) if start < end => {
            let after
                = &existing[end + BLOCK_END.len()..];

            (&existing[..start], after.strip_prefix('\n').unwrap_or(after))
        },

        _ => (existing, ""),
    };

    let user_config
        = format!("{}{}", before, after);

    let replaces_crates_io = user_config.parse::<toml_edit::DocumentMut>().ok()
        .and_then(|document| document.get("source")?.as_table_like()?.get("crates-io").map(|_| ()))
        .is_some();

    if replaces_crates_io {
        return Err(Error::CargoError(format!("{} already replaces the crates-io source; remove that section to let Yarn manage it", CONFIG_PATH)));
    }

    let separator = match before.is_empty() || before.ends_with("\n\n") {
        true => "",
        false if before.ends_with('\n') => "\n",
        false => "\n\n",
    };

    Ok(format!("{}{}{}{}", before, separator, block, after))
}

fn write_if_changed(path: &Path, content: &str) -> Result<(), Error> {
    if path.fs_read_text().ok().as_deref() != Some(content) {
        path.fs_create_parent()?.fs_write_text(content)?;
    }

    Ok(())
}

pub async fn link_island_cargo(project: &Project, install: &Install, island: &ResolvedIsland) -> Result<LinkResult, Error> {
    let mut packages_by_location
        = BTreeMap::new();

    let empty_result = |packages_by_location| LinkResult {
        packages_by_location,
        build_requests: BuildRequests {
            entries: vec![],
            dependencies: BTreeMap::new(),
        },
    };

    let (Some(resolutions), Some(descriptor_to_locator)) = (
        install.install_state.island_normalized_resolutions.get(&island.id),
        install.install_state.island_descriptor_to_locator.get(&island.id),
    ) else {
        return Ok(empty_result(packages_by_location));
    };

    let locators
        = resolutions.keys().collect::<Vec<_>>();

    let indices = locators.iter().enumerate()
        .map(|(index, locator)| (*locator, index))
        .collect::<BTreeMap<_, _>>();

    let mut crates = locators.par_iter()
        .map(|locator| describe_crate(project, install, locator))
        .collect::<Result<Vec<_>, Error>>()?;

    for (index, resolution) in resolutions.values().enumerate() {
        for descriptor in resolution.dependencies.values() {
            let dependency = descriptor_to_locator.get(descriptor)
                .and_then(|locator| indices.get(locator))
                .ok_or_else(|| Error::CargoError(format!("The lockfile doesn't tell what {} resolves to", descriptor.to_file_string())))?;

            crates[index].dependencies.push(*dependency);
        }
    }

    for workspace_ident in &island.workspace_idents {
        let workspace
            = project.workspace_by_ident(workspace_ident)?;

        // Several Cargo workspaces may share an island; each of them
        // gets the part of the graph its own crates depend on
        let mut reachable
            = BTreeSet::new();

        let mut queue = crates.iter().enumerate()
            .filter(|(_, linked)| linked.local_dir.as_ref().is_some_and(|dir| workspace.path.contains(dir)))
            .map(|(index, _)| index)
            .collect::<Vec<_>>();

        while let Some(index) = queue.pop() {
            if reachable.insert(index) {
                queue.extend(crates[index].dependencies.iter().copied());
            }
        }

        let vendor
            = vendor_path(&workspace.path);

        vendor.fs_create_dir_all()?;

        // Crates sharing a name and a version (one from the registry, one
        // from git) can't share a folder
        let mut folders: BTreeMap<String, usize>
            = BTreeMap::new();

        for index in reachable.iter().copied().filter(|index| crates[*index].data.is_some()) {
            let linked
                = &crates[index];

            let mut folder
                = format!("{}-{}", linked.name, linked.version);

            if let (true, LockSource::Git {commit, ..}) = (folders.contains_key(&folder), &linked.source) {
                folder = format!("{}-{}", folder, &commit[..8]);
            }

            folders.insert(folder, index);
        }

        folders.par_iter()
            .map(|(folder, index)| vendor_crate(&crates[*index], &vendor.with_join_str(folder)))
            .collect::<Result<Vec<_>, Error>>()?;

        for (folder, index) in &folders {
            packages_by_location.insert(vendor.relative_to(&project.project_cwd).with_join_str(folder), crates[*index].locator.clone());
        }

        // What the previous installs vendored and isn't needed anymore
        for entry in vendor.fs_read_dir()?.flatten() {
            let name
                = entry.file_name().to_string_lossy().to_string();

            if entry.path().is_dir() && !folders.contains_key(&name) {
                vendor.with_join_str(&name).fs_rm()?;
            }
        }

        let lockfile_path
            = workspace.path.with_join_str(LOCKFILE_NAME);

        // The format of an existing lockfile is kept, so that adopting
        // Yarn doesn't rewrite it
        let lockfile_version = lockfile_path.fs_read_text().ok()
            .and_then(|text| parse_lockfile(&text))
            .and_then(|locked| locked.version)
            .filter(|version| *version >= 3)
            .unwrap_or(DEFAULT_LOCKFILE_VERSION);

        let positions = reachable.iter().enumerate()
            .map(|(position, index)| (*index, position))
            .collect::<BTreeMap<_, _>>();

        let lock_packages = reachable.iter().map(|index| {
            let linked
                = &crates[*index];

            LockPackage {
                name: linked.name.clone(),
                version: linked.version.clone(),
                source: linked.source.clone(),
                checksum: linked.checksum.clone(),
                dependencies: linked.dependencies.iter().map(|dependency| positions[dependency]).collect(),
            }
        }).collect::<Vec<_>>();

        write_if_changed(&lockfile_path, &render_lockfile(&lock_packages, lockfile_version))?;

        let git_sources = reachable.iter()
            .filter_map(|index| match &crates[*index].source {
                LockSource::Git {source, ..} => Some(source.clone()),
                _ => None,
            })
            .collect::<BTreeSet<_>>();

        let config_path
            = workspace.path.with_join_str(CONFIG_PATH);

        let config
            = merge_config(&config_path.fs_read_text().unwrap_or_default(), &render_config_block(&git_sources))?;

        write_if_changed(&config_path, &config)?;

        write_if_changed(&vendor.with_join_str(STATE_MARKER), &island_state(project, &island.id, &island.workspace_idents)?)?;
    }

    Ok(empty_result(packages_by_location))
}

#[cfg(test)]
mod tests {
    use zpm_utils::FromFileString;

    use super::*;

    fn block() -> String {
        render_config_block(&BTreeSet::from([
            CargoGitSource::from_file_string("https://github.com/arcanis/dialoguer.git?branch=mael%2Fconfirm-prompt-height").unwrap(),
            CargoGitSource::from_file_string("https://github.com/arcanis/wax.git").unwrap(),
        ]))
    }

    #[test]
    fn test_config_block() {
        let block
            = block();

        // Cargo must be able to read what we write
        let document
            = block.parse::<toml_edit::DocumentMut>().unwrap();

        let sources
            = document["source"].as_table_like().unwrap();

        assert_eq!(sources.get("crates-io").unwrap()["replace-with"].as_str(), Some("yarn-vendor"));
        assert_eq!(sources.get("yarn-vendor").unwrap()["directory"].as_str(), Some(".cargo/vendor"));

        let dialoguer
            = sources.get("git+https://github.com/arcanis/dialoguer.git?branch=mael%2Fconfirm-prompt-height").unwrap();

        assert_eq!(dialoguer["git"].as_str(), Some("https://github.com/arcanis/dialoguer.git"));
        assert_eq!(dialoguer["branch"].as_str(), Some("mael/confirm-prompt-height"));

        let wax
            = sources.get("git+https://github.com/arcanis/wax.git").unwrap();

        assert!(wax.get("branch").is_none());
    }

    #[test]
    fn test_config_is_merged_with_the_user_configuration() {
        let block
            = block();

        assert_eq!(merge_config("", &block).unwrap(), block);

        let user
            = "[alias]\nt = \"test\"\n";

        let merged
            = merge_config(user, &block).unwrap();

        assert_eq!(merged, format!("{}\n{}", user, block));

        // Installing again replaces the block rather than adding another
        assert_eq!(merge_config(&merged, &block).unwrap(), merged);

        let smaller
            = render_config_block(&BTreeSet::new());

        assert_eq!(merge_config(&format!("{}\n[build]\njobs = 2\n", merged), &smaller).unwrap(), format!("{}\n{}\n[build]\njobs = 2\n", user, smaller));

        merged.parse::<toml_edit::DocumentMut>().unwrap();
    }

    #[test]
    fn test_config_conflicts_are_reported() {
        let user
            = "[source.crates-io]\nreplace-with = \"mirror\"\n";

        assert!(merge_config(user, &block()).is_err());
    }
}
