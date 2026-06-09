use std::collections::{BTreeMap, BTreeSet};

use zpm_primitives::{Ident, Locator, PythonTargetEnv, PythonTargetInput, Reference};
use zpm_utils::{FromFileString, Hash64, Path, System, ToHumanString};

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

fn target_field_matches(target: &Option<String>, current: &Option<String>) -> bool {
    target.is_none() || target == current
}

fn target_matches_current_system(target: &PythonTargetEnv) -> Result<bool, Error> {
    let current_target
        = PythonTargetEnv::from_system(&System::from_current(), PythonTargetInput {
            version: Some(&target.python_version),
            full_version: target.python_full_version.as_deref(),
            implementation_name: target.implementation_name.as_deref(),
            implementation_version: target.implementation_version.as_deref(),
            platform_release: target.platform_release.as_deref(),
            platform_version: target.platform_version.as_deref(),
        }).map_err(|err| Error::InvalidResolution(format!("Invalid current Python target environment: {err}")))?;

    Ok(
        target_field_matches(&target.os_name, &current_target.os_name)
            && target_field_matches(&target.sys_platform, &current_target.sys_platform)
            && target_field_matches(&target.platform_machine, &current_target.platform_machine)
            && target_field_matches(&target.platform_system, &current_target.platform_system)
    )
}

fn target_matches_link_version(target: &PythonTargetEnv, link_version: &str) -> bool {
    target.python_version == link_version
        || target.python_full_version.as_deref() == Some(link_version)
}

fn select_active_fork_id(install: &Install, island: &crate::island::ResolvedIsland) -> Result<Option<Hash64>, Error> {
    let Some(lockfile_island)
        = install.lockfile.islands.get(&island.id) else {
            return Ok(None);
        };

    let mut matches
        = Vec::new();

    for (fork_id, fork) in &lockfile_island.forks {
        let Some(target) = &fork.target else {
            if island.python_link_version.is_none() {
                matches.push(fork_id.clone());
            }
            continue;
        };

        if !target_matches_current_system(target)? {
            continue;
        }

        if let Some(link_version) = &island.python_link_version {
            if !target_matches_link_version(target, link_version) {
                continue;
            }
        }

        matches.push(fork_id.clone());
    }

    match matches.len() {
        0 => {
            let link_version_hint = island.python_link_version.as_ref()
                .map(|link_version| format!(" for python.linkVersion {link_version}"))
                .unwrap_or_default();

            Err(Error::InvalidResolution(format!(
                "No Python fork in island `{}` matches the current system{}",
                island.id,
                link_version_hint,
            )))
        },

        1 => Ok(matches.into_iter().next()),

        _ if island.python_link_version.is_none() => {
            Err(Error::InvalidResolution(format!(
                "Multiple Python forks in island `{}` match the current system; set unstableIslands.{}.python.linkVersion to select one for linking",
                island.id,
                island.id,
            )))
        },

        _ => {
            Err(Error::InvalidResolution(format!(
                "Multiple Python forks in island `{}` match python.linkVersion {}",
                island.id,
                island.python_link_version.as_deref().unwrap_or_default(),
            )))
        },
    }
}

fn workspace_locator_for_fork(workspace_locator: Locator, fork_id: Option<&Hash64>) -> Locator {
    match fork_id {
        Some(fork_id) if fork_id != &crate::lockfile::LockfileIsland::default_fork_id() => {
            workspace_locator.env_qualified_with_hash(fork_id.clone())
        },
        _ => workspace_locator,
    }
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
    let active_fork_id
        = select_active_fork_id(install, island)?;

    for workspace_ident in &island.workspace_idents {
        let workspace
            = project.workspace_by_ident(workspace_ident)?;

        let workspace_locator
            = workspace_locator_for_fork(workspace.locator(), active_fork_id.as_ref());

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lockfile::{LockfileIsland, LockfileIslandFork};

    fn current_target(version: &'static str) -> PythonTargetEnv {
        PythonTargetEnv::from_system(&System::from_current(), PythonTargetInput {
            version: Some(version),
            ..PythonTargetInput::default()
        }).unwrap()
    }

    fn island_with_link_version(link_version: Option<&str>) -> crate::island::ResolvedIsland {
        crate::island::ResolvedIsland {
            id: "python".to_string(),
            workspace_idents: BTreeSet::new(),
            root_descriptors: BTreeSet::new(),
            linker: zpm_config::IslandLinker::Venv,
            python_link_version: link_version.map(|version| version.to_string()),
        }
    }

    fn install_with_targets(targets: Vec<PythonTargetEnv>) -> Install {
        let mut install
            = Install::default();
        let forks
            = targets.into_iter()
                .map(|target| {
                    (target.fork_id(), LockfileIslandFork {
                        target: Some(target),
                        resolutions: BTreeMap::new(),
                    })
                })
                .collect();

        install.lockfile.islands.insert("python".to_string(), LockfileIsland {
            forks,
        });

        install
    }

    #[test]
    fn test_select_active_fork_uses_link_version_when_ambiguous() {
        let target_311
            = current_target("3.11");
        let target_312
            = current_target("3.12");
        let fork_312
            = target_312.fork_id();
        let install
            = install_with_targets(vec![target_311, target_312]);
        let island
            = island_with_link_version(Some("3.12"));

        assert_eq!(Some(fork_312), select_active_fork_id(&install, &island).unwrap());
    }

    #[test]
    fn test_select_active_fork_errors_when_ambiguous_without_link_version() {
        let install
            = install_with_targets(vec![current_target("3.11"), current_target("3.12")]);
        let island
            = island_with_link_version(None);
        let err
            = select_active_fork_id(&install, &island).unwrap_err();
        let Error::InvalidResolution(message) = err else {
            panic!("Expected InvalidResolution, got {err:?}");
        };

        assert!(message.contains("Multiple Python forks"), "{message}");
        assert!(message.contains("python.linkVersion"), "{message}");
    }

    #[test]
    fn test_select_active_fork_keeps_legacy_default_fork() {
        let mut install
            = Install::default();
        install.lockfile.islands.insert("python".to_string(), LockfileIsland::from_resolutions(BTreeMap::new()));
        let island
            = island_with_link_version(None);

        assert_eq!(
            Some(LockfileIsland::default_fork_id()),
            select_active_fork_id(&install, &island).unwrap(),
        );
    }
}
