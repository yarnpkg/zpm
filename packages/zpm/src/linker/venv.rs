use std::collections::{BTreeMap, BTreeSet};

use zpm_primitives::{Ident, Locator, Reference};
use zpm_utils::{FromFileString, Path, ToHumanString};

use crate::{
    build::BuildRequests,
    error::Error,
    fetchers::PackageData,
    install::Install,
    linker::{self, LinkResult},
    project::Project,
};

fn collect_workspace_package_locators(install: &Install, workspace_locator: &Locator) -> Result<BTreeMap<Ident, Locator>, Error> {
    let mut packages
        = BTreeMap::new();

    let mut seen
        = BTreeSet::new();

    let mut queue
        = vec![workspace_locator.clone()];

    while let Some(locator) = queue.pop() {
        if !seen.insert(locator.clone()) {
            continue;
        }

        let resolution
            = install.install_state.resolution_tree.locator_resolutions.get(&locator)
                .unwrap_or_else(|| panic!("Expected resolution entry for {}", locator.to_print_string()));

        for descriptor in resolution.dependencies.values() {
            let dependency_locator
                = install.install_state.resolution_tree.descriptor_to_locator.get(descriptor)
                    .unwrap_or_else(|| panic!("Expected locator for descriptor {}", descriptor.to_print_string()))
                    .clone();

            queue.push(dependency_locator.clone());

            let physical_locator
                = dependency_locator.physical_locator();

            if physical_locator.reference.is_workspace_reference() {
                continue;
            }

            if let Some(existing_locator) = packages.get(&physical_locator.ident) {
                if existing_locator != &physical_locator {
                    return Err(Error::Unsupported);
                }

                continue;
            }

            packages.insert(physical_locator.ident.clone(), physical_locator);
        }
    }

    Ok(packages)
}

fn link_package_into_venv(
    project: &Project,
    install: &Install,
    locator: &Locator,
    site_packages_path: &Path,
    packages_by_location: &mut BTreeMap<Path, Locator>,
) -> Result<(), Error> {
    let package_path
        = site_packages_path
            .with_join_str(&locator.ident.as_str());

    if package_path.fs_exists() {
        package_path.fs_rm()?;
    }

    package_path.fs_create_parent()?;

    let rel_path
        = package_path.relative_to(&project.project_cwd);

    packages_by_location.insert(rel_path, locator.clone());

    let package_data
        = install.package_data.get(&locator.physical_locator());

    match package_data {
        Some(PackageData::Abstract) => {
            Err(Error::Unsupported)
        },

        Some(PackageData::Local {package_directory, ..}) => {
            package_path.fs_symlink(package_directory)?;
            Ok(())
        },

        Some(package_data @ PackageData::Zip {..}) => {
            package_path.fs_create_dir_all()?;
            linker::helpers::fs_extract_archive(&package_path, package_data)?;
            Ok(())
        },

        Some(PackageData::MissingZip {..}) => {
            Ok(())
        },

        None => match &locator.reference {
            Reference::Link(params) if params.path.starts_with('/') => {
                let target_path
                    = Path::from_file_string(&params.path)?;

                package_path.fs_symlink(&target_path)?;

                Ok(())
            },

            _ => {
                unreachable!("Expected package data for {}", locator.to_print_string());
            },
        },
    }
}

fn get_workspace_site_packages_path(workspace_path: &Path) -> Path {
    workspace_path
        .with_join_str(".venv")
        .with_join_str("lib")
        .with_join_str("site-packages")
}

pub async fn link_island_venv(
    project: &Project,
    install: &Install,
    island: &crate::island::ResolvedIsland,
) -> Result<LinkResult, Error> {
    let mut packages_by_location
        = BTreeMap::new();

    for workspace_ident in &island.workspace_idents {
        let workspace
            = project.workspace_by_ident(workspace_ident)?;

        let workspace_locator
            = workspace.locator();

        packages_by_location.insert(workspace.rel_path.clone(), workspace_locator.clone());

        let workspace_path
            = project.project_cwd
                .with_join(&workspace.rel_path);

        let site_packages_path
            = get_workspace_site_packages_path(&workspace_path);

        if site_packages_path.fs_exists() {
            site_packages_path.fs_rm()?;
        }

        site_packages_path.fs_create_dir_all()?;

        let package_locators
            = collect_workspace_package_locators(install, &workspace_locator)?;

        for package_locator in package_locators.values() {
            link_package_into_venv(
                project,
                install,
                package_locator,
                &site_packages_path,
                &mut packages_by_location,
            )?;
        }
    }

    Ok(LinkResult {
        packages_by_location,
        build_requests: BuildRequests {
            entries: vec![],
            dependencies: BTreeMap::new(),
        },
    })
}
