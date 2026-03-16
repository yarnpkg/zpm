use std::{collections::{BTreeMap, BTreeSet}, fs::Permissions, os::unix::fs::PermissionsExt, time::{SystemTime, UNIX_EPOCH}, vec};

use zpm_formats::iter_ext::IterExt;
use zpm_parsers::JsonDocument;
use zpm_primitives::{Descriptor, FilterDescriptor, Locator};
use zpm_utils::{Path, PathError, System, ToFileString};
use sha2::{Digest, Sha512};
use serde::{Deserialize, Serialize};
use serde_with::serde_as;
use hex;

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
    dependencies_meta: Vec<(FilterDescriptor, PackageMeta)>,
}

impl TopLevelConfiguration {
    pub fn from_project(project: &Project) -> Vec<(FilterDescriptor, PackageMeta)> {
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
    let ready_path = destination
        .with_join_str(".ready");

    if !ready_path.fs_exists() && !matches!(package_data, &PackageData::MissingZip {..}) {
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

            target_path
                .fs_create_parent()?
                .fs_write(&entry.data)?
                .fs_set_permissions(Permissions::from_mode(entry.mode as u32))?;
        }

        ready_path
            .fs_write(vec![])?;

        Ok(true)
    } else {
        Ok(false)
    }
}

// Helper function to compute SHA512 hash and return as hex string
fn compute_sha512_hex(input: &str) -> String {
    let mut hasher = Sha512::new();
    hasher.update(input.as_bytes());
    hex::encode(hasher.finalize())
}

// Generates a Yarn Berry-compatible hash. Used for Sharp packages
pub fn yarn_berry_hash(locator: &Locator) -> Result<String, Error> {
    let package_version = locator.reference.to_file_string();

    // Extract scope without '@' prefix, or empty string if no scope
    let package_scope = locator.ident.scope()
        .and_then(|scope| scope.strip_prefix('@'))
        .unwrap_or("");

    // Step 1: Hash the package identifier (scope + name)
    let package_identifier = format!("{}{}", package_scope, locator.ident.name());
    let identifier_hash = compute_sha512_hex(&package_identifier);

    // Step 2: Hash the combination of identifier hash and version
    let combined_input = format!("{}{}", identifier_hash, package_version);
    let final_hash = compute_sha512_hex(&combined_input);

    // Return first 10 characters to match Yarn Berry's hash length
    Ok(final_hash[..10].to_string())
}

pub fn fs_materialize_unplugged_from_global_cache(project: &Project, locator: &Locator, dest_wrapper: &Path, dest_package_root: &Path, package_data: &PackageData) -> Result<bool, Error> {
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = (project, locator, dest_wrapper);
        return fs_extract_archive(dest_package_root, package_data);
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        let dest_ready = dest_package_root
            .with_join_str(".ready");

        if dest_ready.fs_exists() {
            return Ok(false);
        }

        let PackageData::Zip {..} = package_data else {
            return Ok(false);
        };

        let global_base = project
            .global_unplugged_path();

        global_base
            .fs_create_dir_all()?;

        let physical = locator
            .physical_locator();

        let global_wrapper_name = format!(
            "{}-{}-{}",
            physical.ident.slug(),
            physical.reference.slug(),
            yarn_berry_hash(&physical)?,
        );

        let global_wrapper = global_base
            .with_join_str(&global_wrapper_name);

        let package_subpath = package_data
            .package_subpath();

        let global_package_root = global_wrapper
            .with_join(&package_subpath);

        let global_ready = global_package_root
            .with_join_str(".ready");

        if !global_ready.fs_exists() {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();

            let tmp_wrapper = global_base.with_join_str(format!(
                ".{}.tmp-{}-{}",
                global_wrapper_name,
                std::process::id(),
                nonce,
            ));

            let tmp_package_root = tmp_wrapper
                .with_join(&package_subpath);

            if fs_extract_archive(&tmp_package_root, package_data).is_ok() {
                let _ = tmp_wrapper
                    .fs_concurrent_move(&global_wrapper);
            }

            if !global_ready.fs_exists() {
                let _ = tmp_wrapper.fs_rm();
                return fs_extract_archive(dest_package_root, package_data);
            }

            let _ = tmp_wrapper.fs_rm();
        }

        if dest_wrapper.fs_exists() && !dest_ready.fs_exists() {
            dest_wrapper.fs_rm()?;
        }

        dest_wrapper
            .fs_create_parent()?;

        match global_wrapper.fs_clonefile(dest_wrapper) {
            Ok(_) => Ok(true),
            Err(_) => {
                if dest_wrapper.fs_exists() {
                    let _ = dest_wrapper.fs_rm();
                }

                fs_extract_archive(dest_package_root, package_data)
            },
        }
    }
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

                if let Some(dependency_entry_idx) = package_build_entries.get(dependency_locator) {
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
    pub build_commands: Option<Vec<build::Command>>,
}

pub fn get_package_internal_info(project: &Project, install: &Install, dependencies_meta: &Vec<(FilterDescriptor, PackageMeta)>, locator: &Locator, resolution: &Resolution, physical_package_data: &PackageData) -> PackageBuildInfo {
    // The package meta is based on the top-level configuration extracted
    // from the `dependenciesMeta` field.
    //
    let package_meta
        = dependencies_meta.iter()
            .find(|(selector, _)| selector.check(&locator.ident, &resolution.version))
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
    let should_build_if_compatible
        = package_flags.build_commands.len() > 0
            && (locator.reference.is_workspace_reference() || package_meta.built.unwrap_or(project.config.settings.enable_scripts.value));

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

    let must_build
        = should_build_if_compatible && is_compatible;

    let build_commands
        = must_build.then_some(package_flags.build_commands.clone());

    PackageBuildInfo {
        must_extract,
        build_commands,
    }
}
