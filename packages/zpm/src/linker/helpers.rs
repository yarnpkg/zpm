use std::{collections::{BTreeMap, BTreeSet, HashMap}, fs::Permissions, os::unix::fs::PermissionsExt, vec};

use zpm_formats::iter_ext::IterExt;
use zpm_primitives::{Descriptor, FilterDescriptor, Ident, Locator};
use zpm_utils::{Path, PathError, ToFileString};
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use serde_with::serde_as;

use crate::{
    build,
    error::Error,
    fetchers::PackageData,
    install::Install,
    project::Project,
    resolvers::Resolution,
    system,
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
#[derive(Debug, Default, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TopLevelConfiguration {
    #[serde(default)]
    #[serde_as(as = "BTreeMap<_, _>")]
    dependencies_meta: Vec<(FilterDescriptor, PackageMeta)>,
}

impl TopLevelConfiguration {
    pub fn from_project(project: &Project) -> HashMap<Ident, Vec<(FilterDescriptor, PackageMeta)>> {
        project.manifest_path()
            .if_exists()
            .and_then(|path| path.fs_read_text().ok()).map(|data| sonic_rs::from_str::<TopLevelConfiguration>(&data).unwrap().dependencies_meta)
            .unwrap_or_default()
            .into_iter()
            .map(|(filter, meta)| (filter.ident().clone(), (filter, meta)))
            .into_group_map()
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
                .strip_path_prefix(package_subpath.to_file_string())
                .collect::<Vec<_>>();

        for entry in entries {
            let target_path = destination
                .with_join_str(&entry.name);

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

pub fn get_package_internal_info(project: &Project, install: &Install, dependencies_meta: &HashMap<Ident, Vec<(FilterDescriptor, PackageMeta)>>, locator: &Locator, resolution: &Resolution, physical_package_data: &PackageData) -> PackageBuildInfo {
    // The package meta is based on the top-level configuration extracted
    // from the `dependenciesMeta` field.
    //
    let package_meta = dependencies_meta
        .get(&locator.ident)
        .and_then(|meta_list| {
            meta_list.iter().find_map(|(selector, meta)| match selector {
                FilterDescriptor::Range(params) => params.range.check(&resolution.version).then_some(meta),
                FilterDescriptor::Ident(_) => Some(meta),
            })
        })
        .cloned()
        .unwrap_or_default();

    // The package flags are based on the actual package content. The flags
    // should always be the same for the same package, so we keep them in
    // the install state so we don't have to recompute them at every install.
    //
    let package_flags = &install.lockfile.entries
        .get(&locator.physical_locator())
        .expect("Expected package flags to be set")
        .flags;

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
        .validate_system(&system::System::from_current());

    let must_build
        = should_build_if_compatible && is_compatible;

    let build_commands
        = must_build.then_some(package_flags.build_commands.clone());

    PackageBuildInfo {
        must_extract,
        build_commands,
    }
}
