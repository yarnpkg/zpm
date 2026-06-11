use std::collections::{BTreeMap, BTreeSet};

use zpm_primitives::{Ident, Locator, PythonTargetEnv, PythonTargetInput, Reference};
use zpm_utils::{FromFileString, Hash64, Path, System, ToFileString, ToHumanString};

use crate::{
    build::BuildRequests,
    error::Error,
    fetchers::PackageData,
    install::Install,
    linker::{self, LinkResult},
    project::Project,
};

#[derive(Clone, Debug, PartialEq, Eq)]
struct ActivePythonFork {
    id: Option<Hash64>,
    target: Option<PythonTargetEnv>,
}

fn is_python_builtin_ident(ident: &Ident) -> bool {
    ident.as_str() == "@yarnpkg/python"
        || ident.as_str().starts_with("@yarnpkg/python-")
}

fn is_python_variant_ident(ident: &Ident) -> bool {
    ident.as_str().starts_with("@yarnpkg/python-")
}

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

        for descriptor in resolution.dependencies.values().chain(resolution.variants.iter()) {
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
            && target_field_matches(&target.libc, &current_target.libc)
    )
}

fn target_matches_link_version(target: &PythonTargetEnv, link_version: &str) -> bool {
    target.python_version == link_version
        || target.python_full_version.as_deref() == Some(link_version)
}

fn select_active_fork(install: &Install, island: &crate::island::ResolvedIsland) -> Result<ActivePythonFork, Error> {
    let Some(lockfile_island)
        = install.lockfile.islands.get(&island.id) else {
            return Ok(ActivePythonFork {
                id: None,
                target: None,
            });
        };

    let mut matches
        = Vec::new();

    for (fork_id, fork) in &lockfile_island.forks {
        let Some(target) = &fork.target else {
            if island.python_link_version.is_none() {
                matches.push(ActivePythonFork {
                    id: Some(fork_id.clone()),
                    target: None,
                });
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

        matches.push(ActivePythonFork {
            id: Some(fork_id.clone()),
            target: Some(target.clone()),
        });
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

        1 => Ok(matches.into_iter().next().unwrap()),

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

fn python_lib_dir_name(target: Option<&PythonTargetEnv>) -> String {
    target
        .map(|target| format!("python{}", target.python_version))
        .unwrap_or_else(|| "python".to_string())
}

fn get_workspace_venv_path(workspace_path: &Path) -> Path {
    workspace_path
        .with_join_str(".venv")
}

fn get_workspace_site_packages_path(workspace_path: &Path, target: Option<&PythonTargetEnv>) -> Path {
    get_workspace_venv_path(workspace_path)
        .with_join_str("lib")
        .with_join_str(python_lib_dir_name(target))
        .with_join_str("site-packages")
}

fn get_legacy_site_packages_path(workspace_path: &Path) -> Path {
    get_workspace_venv_path(workspace_path)
        .with_join_str("lib")
        .with_join_str("site-packages")
}

fn prepare_venv_root(venv_path: &Path) -> Result<(), Error> {
    venv_path.fs_create_dir_all()?;
    venv_path
        .with_join_str(".gitignore")
        .fs_write_text("*\n")?;

    Ok(())
}

fn recreate_legacy_site_packages_alias(workspace_path: &Path, site_packages_path: &Path) -> Result<(), Error> {
    let legacy_site_packages_path
        = get_legacy_site_packages_path(workspace_path);

    if legacy_site_packages_path.fs_exists() || legacy_site_packages_path.fs_is_symlink() {
        legacy_site_packages_path.fs_rm()?;
    }

    legacy_site_packages_path.fs_create_parent()?;
    legacy_site_packages_path.fs_symlink(site_packages_path)?;

    Ok(())
}

fn find_managed_python_locator(package_locators: &BTreeMap<Ident, Locator>) -> Option<&Locator> {
    package_locators
        .values()
        .find(|locator| is_python_variant_ident(&locator.ident))
}

fn find_python_executable_path(python_home: &Path, target: Option<&PythonTargetEnv>) -> Option<Path> {
    let mut candidates
        = Vec::new();

    if let Some(target) = target {
        candidates.push(format!("bin/python{}", target.python_version));
    }

    candidates.extend(["bin/python3".to_string(), "bin/python".to_string()]);

    candidates.into_iter()
        .map(|candidate| python_home.with_join_str(candidate))
        .find(|candidate| candidate.fs_exists())
}

fn write_pyvenv_cfg(venv_path: &Path, python_executable: &Path, target: Option<&PythonTargetEnv>) -> Result<(), Error> {
    let home
        = python_executable.dirname()
            .unwrap_or_else(|| python_executable.clone());

    let version
        = target
            .map(|target| target.python_full_version.as_deref().unwrap_or(&target.python_version))
            .unwrap_or("unknown");

    venv_path
        .with_join_str("pyvenv.cfg")
        .fs_write_text(format!(
            "home = {}\nimplementation = CPython\nversion_info = {}\ninclude-system-site-packages = false\nversion = {}\nexecutable = {}\n",
            home.to_file_string(),
            version,
            version,
            python_executable.to_file_string(),
        ))?;

    Ok(())
}

fn link_venv_python_binary(venv_path: &Path, python_executable: &Path, target: Option<&PythonTargetEnv>) -> Result<(), Error> {
    let bin_path
        = venv_path.with_join_str("bin");

    bin_path.fs_create_dir_all()?;

    let mut names
        = vec!["python".to_string(), "python3".to_string()];

    if let Some(target) = target {
        names.push(format!("python{}", target.python_version));
    }

    names.sort();
    names.dedup();

    for name in names {
        let link_path
            = bin_path.with_join_str(name);

        if link_path.fs_exists() || link_path.fs_is_symlink() {
            link_path.fs_rm()?;
        }

        link_path.fs_symlink(python_executable)?;
    }

    Ok(())
}

fn materialize_managed_python(
    install: &Install,
    locator: &Locator,
    venv_path: &Path,
    target: Option<&PythonTargetEnv>,
) -> Result<(), Error> {
    let python_home
        = venv_path.with_join_str(".python");

    if python_home.fs_exists() || python_home.fs_is_symlink() {
        python_home.fs_rm()?;
    }

    python_home.fs_create_parent()?;

    let package_data
        = install.package_data.get(&locator.physical_locator())
            .unwrap_or_else(|| panic!("Expected package data for {}", locator.to_print_string()));

    match package_data {
        PackageData::Zip {..} => {
            python_home.fs_create_dir_all()?;
            linker::helpers::fs_extract_archive(&python_home, package_data)?;
        },

        PackageData::Local {package_directory, ..} => {
            python_home.fs_symlink(package_directory)?;
        },

        PackageData::MissingZip {..} | PackageData::Abstract => {
            return Err(Error::Unsupported);
        },
    }

    let python_executable
        = find_python_executable_path(&python_home, target)
            .ok_or_else(|| Error::InvalidResolution(format!("Unable to find a Python executable in {}", python_home.to_file_string())))?;

    write_pyvenv_cfg(venv_path, &python_executable, target)?;
    link_venv_python_binary(venv_path, &python_executable, target)?;

    Ok(())
}

pub async fn link_island_venv(
    project: &Project,
    install: &Install,
    island: &crate::island::ResolvedIsland,
) -> Result<LinkResult, Error> {
    let mut packages_by_location
        = BTreeMap::new();
    let active_fork
        = select_active_fork(install, island)?;

    for workspace_ident in &island.workspace_idents {
        let workspace
            = project.workspace_by_ident(workspace_ident)?;

        let workspace_locator
            = workspace_locator_for_fork(workspace.locator(), active_fork.id.as_ref());

        packages_by_location.insert(workspace.rel_path.clone(), workspace_locator.clone());

        let workspace_path
            = project.project_cwd
                .with_join(&workspace.rel_path);

        let package_locators
            = collect_workspace_package_locators(install, &workspace_locator)?;

        let venv_path
            = get_workspace_venv_path(&workspace_path);
        prepare_venv_root(&venv_path)?;

        let site_packages_path
            = get_workspace_site_packages_path(&workspace_path, active_fork.target.as_ref());

        if site_packages_path.fs_exists() {
            site_packages_path.fs_rm()?;
        }

        site_packages_path.fs_create_dir_all()?;
        recreate_legacy_site_packages_alias(&workspace_path, &site_packages_path)?;

        if let Some(python_locator) = find_managed_python_locator(&package_locators) {
            materialize_managed_python(
                install,
                python_locator,
                &venv_path,
                active_fork.target.as_ref(),
            )?;
        }

        for package_locator in package_locators.values() {
            if is_python_builtin_ident(&package_locator.ident) {
                continue;
            }

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
            = install_with_targets(vec![target_311, target_312.clone()]);
        let island
            = island_with_link_version(Some("3.12"));

        assert_eq!(
            ActivePythonFork {
                id: Some(fork_312),
                target: Some(target_312),
            },
            select_active_fork(&install, &island).unwrap(),
        );
    }

    #[test]
    fn test_select_active_fork_errors_when_ambiguous_without_link_version() {
        let install
            = install_with_targets(vec![current_target("3.11"), current_target("3.12")]);
        let island
            = island_with_link_version(None);
        let err
            = select_active_fork(&install, &island).unwrap_err();
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
            ActivePythonFork {
                id: Some(LockfileIsland::default_fork_id()),
                target: None,
            },
            select_active_fork(&install, &island).unwrap(),
        );
    }
}
