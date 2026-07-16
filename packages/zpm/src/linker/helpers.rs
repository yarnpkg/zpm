use std::{collections::{BTreeMap, BTreeSet}, vec};

use zpm_formats::iter_ext::IterExt;
use zpm_parsers::JsonDocument;
use zpm_primitives::{Descriptor, VersionFilter, Locator, Reference};
use zpm_utils::{Path, PathError, System};
use serde::{Deserialize, Serialize};
use serde_with::serde_as;

use crate::{
    build,
    error::Error,
    fetchers::PackageData,
    install::Install,
    project::Project,
    resolvers::Resolution,
};

#[derive(Debug, Default, Clone, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageMeta {
    #[serde(default, skip_serializing_if = "zpm_utils::is_default")]
    pub built: Option<bool>,

    #[serde(default, skip_serializing_if = "zpm_utils::is_default")]
    pub unplugged: Option<bool>,
}

#[serde_as]
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TopLevelConfiguration {
    #[serde(default)]
    #[serde_as(as = "BTreeMap<_, _>")]
    dependencies_meta: Vec<(VersionFilter, PackageMeta)>,
}

impl TopLevelConfiguration {
    pub fn from_project(project: &Project) -> Vec<(VersionFilter, PackageMeta)> {
        project.manifest_path()
            .if_exists()
            .and_then(|path| path.fs_read_text().ok()).map(|data| JsonDocument::hydrate_from_str::<TopLevelConfiguration>(&data).unwrap().dependencies_meta)
            .unwrap_or_default()
    }
}

pub fn fs_remove_nm(nm_path: Path) -> Result<(), Error> {
    let entries = nm_path.fs_read_dir();

    match entries {
        Err(PathError::IoError {inner, ..}) if inner.kind() == std::io::ErrorKind::NotFound
            => Ok(()),

        Err(error)
            => Err(error.into()),

        Ok(entries) => {
            let mut has_dot_entries = false;

            for entry in entries.flatten() {
                let path
                    = Path::try_from(entry.path())?;

                let basename = path.basename()
                    .unwrap();

                if basename.starts_with(".") && basename != ".bin" && path.fs_is_dir() {
                    has_dot_entries = true;
                    continue;
                }

                path.fs_rm()
                    .unwrap();
            }

            if !has_dot_entries {
                nm_path.fs_rm()?;
            }

            Ok(())
        },
    }
}

pub fn fs_extract_archive(destination: &Path, package_data: &PackageData) -> Result<bool, Error> {
    fs_extract_archive_impl(destination, package_data, ExtractMode::Classic)
}

/// Like [`fs_extract_archive`] but hardlinks each file through a
/// content-addressed index at `index_root` so identical contents share
/// one inode across packages and projects. Modified hardlinks are
/// detected via the mtime stamp and repaired in place.
pub fn fs_extract_archive_with_cas(destination: &Path, package_data: &PackageData, index_root: &Path) -> Result<bool, Error> {
    fs_extract_archive_impl(destination, package_data, ExtractMode::Cas { index_root })
}

/// Project-local dedup with no side-channel index. The first
/// destination for a given content hash is written normally and
/// recorded in `local_index`; later destinations hardlink to it.
pub fn fs_extract_archive_with_local_dedup(
    destination: &Path,
    package_data: &PackageData,
    local_index: &mut BTreeMap<zpm_utils::Hash64, Path>,
) -> Result<bool, Error> {
    fs_extract_archive_impl(destination, package_data, ExtractMode::LocalDedup { local_index })
}

enum ExtractMode<'a> {
    Classic,
    Cas { index_root: &'a Path },
    LocalDedup { local_index: &'a mut BTreeMap<zpm_utils::Hash64, Path> },
}

fn fs_extract_archive_impl(destination: &Path, package_data: &PackageData, mut mode: ExtractMode<'_>) -> Result<bool, Error> {
    let ready_path = destination
        .with_join_str(".ready");

    let already_extracted = ready_path.fs_exists();

    // Classic mode trusts the .ready marker and skips the zip walk.
    // CAS / local-dedup re-walk so out-of-band edits can be repaired.
    let skip_for_classic = matches!(mode, ExtractMode::Classic) && already_extracted;
    if skip_for_classic || matches!(package_data, &PackageData::MissingZip {..}) {
        return Ok(false);
    }

    let package_subpath
        = package_data.package_subpath();

    let package_bytes = match package_data {
        PackageData::Zip {archive_path, ..} => archive_path.fs_read()?,
        _ => panic!("Expected a zip archive"),
    };

    let entries
        = zpm_formats::zip::entries_from_zip(&package_bytes)?
            .into_iter()
            .strip_path_prefix(&package_subpath)
            .collect::<Vec<_>>();

    for entry in entries {
        let target_path = destination
            .with_join(&entry.name);

        target_path.fs_create_parent()?;

        match &mut mode {
            ExtractMode::Cas { index_root } => {
                link_into_cas(&target_path, &entry.data, entry.mode as u32, index_root)?;
            },
            ExtractMode::LocalDedup { local_index } => {
                link_into_local_index(&target_path, &entry.data, entry.mode as u32, local_index)?;
            },
            ExtractMode::Classic => {
                target_path
                    .fs_write(&entry.data)?
                    .fs_set_mode(entry.mode as u32)?;
            },
        }
    }

    if !already_extracted {
        ready_path
            .fs_write(vec![])?;
    }

    Ok(!already_extracted)
}

/// Ensures `target` is a hardlink to `source` (no-op if they already
/// share an inode). `source` must exist.
fn ensure_hardlink(target: &Path, source: &Path) -> Result<(), Error> {
    #[cfg(unix)]
    let already_linked = {
        use std::os::unix::fs::MetadataExt;

        let dest_meta = target.fs_symlink_metadata().ok();
        let source_meta = source.fs_metadata().ok();
        match (&dest_meta, &source_meta) {
            (Some(d), Some(s)) => d.dev() == s.dev() && d.ino() == s.ino(),
            _ => false,
        }
    };

    #[cfg(windows)]
    let already_linked = false;

    if already_linked {
        return Ok(());
    }

    if target.fs_exists() {
        target.fs_rm_file().ok();
    }

    std::fs::hard_link(source.to_path_buf(), target.to_path_buf())
        .map_err(zpm_utils::PathError::from)?;

    Ok(())
}

/// Writes `data` at `target` with `mode_bits` permissions, overwriting
/// if present.
fn write_canonical(target: &Path, data: &[u8], mode_bits: u32) -> Result<(), Error> {
    if target.fs_exists() {
        target.fs_rm_file().ok();
    }

    target
        .fs_write(data)?
        .fs_set_mode(mode_bits)?;

    Ok(())
}

fn link_into_local_index(
    target_path: &Path,
    data: &[u8],
    mode: u32,
    local_index: &mut BTreeMap<zpm_utils::Hash64, Path>,
) -> Result<(), Error> {
    let mode_bits = mode & 0o777;
    let hash = zpm_utils::Hash64::from_data(data);

    // Fall through to write_canonical when the recorded path is gone;
    // that re-registers a new home for this hash.
    let canonical_path = local_index.get(&hash).cloned()
        .filter(|p| p.fs_exists());

    let Some(canonical_path) = canonical_path else {
        write_canonical(target_path, data, mode_bits)?;
        local_index.insert(hash, target_path.clone());
        return Ok(());
    };

    ensure_hardlink(target_path, &canonical_path)
}

/// Stamped on every CAS entry. An mtime that doesn't match flags
/// out-of-band modification and triggers a repair on the next install.
const CAS_SAFE_TIME_SECS: i64 = 456_789_000;

const CAS_DEFAULT_MODE: u32 = 0o644;

fn link_into_cas(target_path: &Path, data: &[u8], mode: u32, index_root: &Path) -> Result<(), Error> {
    use sha1::{Digest, Sha1};

    let mode_bits = mode & 0o777;

    let hash = {
        let mut hasher = Sha1::new();
        hasher.update(data);
        hex::encode(hasher.finalize())
    };

    let index_filename = if mode_bits == CAS_DEFAULT_MODE {
        format!("{}.dat", hash)
    } else {
        format!("{}{:o}.dat", hash, mode_bits)
    };

    let index_dir = index_root
        .with_join_str(&hash[..2]);
    let index_path = index_dir
        .with_join_str(&index_filename);

    index_dir.fs_create_dir_all()?;

    let mut needs_rewrite = !index_path.fs_exists();

    if !needs_rewrite {
        // mtime != SAFE_TIME ⇒ external write since the last install.
        if let Ok(metadata) = index_path.fs_metadata() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                if metadata.mtime() != CAS_SAFE_TIME_SECS {
                    needs_rewrite = true;
                }
            }

            #[cfg(windows)]
            {
            let safe_time = std::time::UNIX_EPOCH + std::time::Duration::from_secs(CAS_SAFE_TIME_SECS as u64);
            if metadata.modified().ok() != Some(safe_time) {
                needs_rewrite = true;
            }
            }
        }
    }

    if needs_rewrite {
        // Write through the existing path: cross-project hardlinks
        // inherit the repair without losing inode identity.
        index_path.fs_write(data)?;
        index_path.fs_set_mode(mode_bits)?;
        set_safe_mtime(&index_path)?;
    }

    ensure_hardlink(target_path, &index_path)
}

fn set_safe_mtime(path: &Path) -> Result<(), Error> {
    let safe_time = std::time::UNIX_EPOCH + std::time::Duration::from_secs(CAS_SAFE_TIME_SECS as u64);

    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(path.to_path_buf())
        .map_err(zpm_utils::PathError::from)?;
    file.set_modified(safe_time).map_err(zpm_utils::PathError::from)?;
    Ok(())
}

pub fn populate_build_entry_dependencies(package_build_entries: &BTreeMap<Locator, usize>, locator_resolutions: &BTreeMap<Locator, Resolution>, descriptor_to_locator: &BTreeMap<Descriptor, Locator>) -> Result<BTreeMap<usize, BTreeSet<usize>>, Error> {
    let mut package_build_dependencies
        = BTreeMap::new();

    for locator in package_build_entries.keys() {
        let mut build_dependencies
            = BTreeSet::new();

        let mut queue
            = vec![locator.clone()];
        let mut seen
            = BTreeSet::new();

        seen.insert(locator.clone());

        while let Some(locator) = queue.pop() {
            let resolution = locator_resolutions.get(&locator)
                .expect("Failed to find locator resolution");

            for dependency in resolution.dependencies.values() {
                let dependency_locator = descriptor_to_locator.get(dependency)
                    .expect("Failed to find dependency locator");

                if !seen.insert(dependency_locator.clone()) {
                    continue;
                }

                let dependency_physical_locator
                    = dependency_locator.physical_locator();

                if let Some(dependency_entry_idx) = package_build_entries.get(&dependency_physical_locator) {
                    build_dependencies.insert(*dependency_entry_idx);
                }

                queue.push(dependency_locator.clone());
            }
        }

        let entry_idx = package_build_entries.get(locator)
            .expect("Failed to find build entry index");

        package_build_dependencies.insert(*entry_idx, build_dependencies);
    }

    Ok(package_build_dependencies)
}
pub struct PackageBuildInfo {
    pub must_extract: bool,
    pub build_step: build::BuildStep,
}

pub fn get_package_internal_info(project: &Project, install: &Install, dependencies_meta: &Vec<(VersionFilter, PackageMeta)>, locator: &Locator, resolution: &Resolution, physical_package_data: &PackageData) -> PackageBuildInfo {
    // The package meta is based on the top-level configuration extracted
    // from the `dependenciesMeta` field.
    //
    let package_ident = match &locator.reference {
        Reference::Registry(params) => &params.ident,
        _ => &locator.ident,
    };

    let package_meta
        = dependencies_meta.iter()
            .find(|(selector, _)| selector.check(package_ident, &resolution.version))
            .map(|(_, meta)| meta)
            .cloned()
            .unwrap_or_default();

    // The package flags are based on the actual package content. The flags
    // should always be the same for the same package, so we keep them in
    // the install state so we don't have to recompute them at every install.
    //
    let package_flags = &install.install_state.content_flags
        .get(&locator.physical_locator())
        .expect("Expected package flags to be set");

    // We don't take into account `is_compatible` here, as it may change
    // depending on the system and we don't want the paths encoded in the
    // .pnp.cjs file to change depending on the system.
    let has_build_commands = package_flags.build_commands.len() > 0;
    let scripts_allowed_by_meta = package_meta.built.unwrap_or(project.config.settings.enable_scripts.value);
    let scripts_can_run
        = locator.reference.is_workspace_reference() || scripts_allowed_by_meta;
    let should_build_if_compatible
        = has_build_commands && scripts_can_run;

    // Optional dependencies baked by zip archives are always extracted,
    // as we have no way to know whether they would be extracted if we
    // were to download them (this may change depending on the package's
    // files).
    let is_optional
        = install.install_state.resolution_tree.optional_builds.contains(locator);

    let is_baked_by_zip
        = matches!(physical_package_data, PackageData::Zip {..} | PackageData::MissingZip {..});

    let must_extract =
        (is_optional && is_baked_by_zip) || package_meta.unplugged.or(package_flags.prefer_extracted)
            .unwrap_or_else(|| should_build_if_compatible || package_flags.suggest_extracted);

    // We don't need to run the build if the package was marked as
    // incompatible with the current system (even if the package isn't
    // marked as optional).
    let is_compatible = resolution.requirements
        .validate_system(&System::from_current());

    let build_step = if !has_build_commands {
        build::BuildStep::Commands(Vec::new())
    } else if !is_compatible {
        build::BuildStep::Skip(build::BuildSkip::Incompatible)
    } else if !scripts_can_run {
        build::BuildStep::Skip(build::BuildSkip::Disabled)
    } else {
        build::BuildStep::Commands(package_flags.build_commands.clone())
    };

    PackageBuildInfo {
        must_extract,
        build_step,
    }
}
