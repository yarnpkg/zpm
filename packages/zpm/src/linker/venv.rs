use std::collections::{BTreeMap, BTreeSet};

use zpm_primitives::{Ident, Locator, PythonTargetEnv, Reference};
use zpm_utils::{FromFileString, Hash64, Path, ToFileString, ToHumanString};

use crate::{
    build::BuildRequests,
    builtins::python::{is_python_ident as is_python_builtin_ident, is_python_variant_ident},
    content_flags::{self, Binary},
    error::Error,
    fetchers::PackageData,
    install::Install,
    linker::{self, LinkResult},
    prepare,
    project::Project,
    script,
};

const PYTHON_ENTRY_POINTS_MANIFEST: &str = ".zpm-python-entry-points";

#[derive(Clone, Debug, PartialEq, Eq)]
struct ActivePythonFork {
    id: Option<Hash64>,
    target: Option<PythonTargetEnv>,
}

fn collect_workspace_package_locators(
    install: &Install,
    workspace_locator: &Locator,
) -> Result<(BTreeMap<Ident, Locator>, BTreeSet<Locator>), Error> {
    let mut packages
        = BTreeMap::new();
    let mut workspaces
        = BTreeSet::new();

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
                workspaces.insert(physical_locator);
                continue;
            }

            let install_locator = if matches!(
                &physical_locator.reference,
                Reference::PypiShorthand(_) | Reference::PypiRegistry(_)
            ) {
                dependency_locator
            } else {
                physical_locator
            };

            if let Some(existing_locator) = packages.get(&install_locator.ident) {
                if existing_locator != &install_locator {
                    return Err(Error::Unsupported);
                }

                continue;
            }

            packages.insert(install_locator.ident.clone(), install_locator);
        }
    }

    Ok((packages, workspaces))
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

        if !crate::pypi::target_matches_current_system(target)? {
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
        = install.package_data.get(locator)
            .or_else(|| install.package_data.get(&locator.physical_locator()));

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

fn get_venv_bin_path(venv_path: &Path) -> Path {
    if cfg!(windows) {
        venv_path.with_join_str("Scripts")
    } else {
        venv_path.with_join_str("bin")
    }
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

fn remove_inactive_python_lib_dirs(workspace_path: &Path, target: Option<&PythonTargetEnv>) -> Result<(), Error> {
    let lib_path = get_workspace_venv_path(workspace_path).with_join_str("lib");
    let active_dir_name = python_lib_dir_name(target);

    let Ok(entries) = lib_path.fs_read_dir() else {
        return Ok(());
    };

    for entry in entries {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }

        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("python") && name != active_dir_name {
            lib_path.with_join_str(&name).fs_rm()?;
        }
    }

    Ok(())
}

fn prepare_venv_root(venv_path: &Path) -> Result<(), Error> {
    venv_path.fs_create_dir_all()?;
    venv_path
        .with_join_str(".gitignore")
        .fs_write_text("*\n")?;

    Ok(())
}

fn is_safe_python_entry_point_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains('/')
        && !name.contains('\\')
        && !name.contains('\n')
        && !name.contains('\r')
}

fn python_entry_point_path(bin_path: &Path, name: &str) -> Path {
    if cfg!(windows) {
        bin_path.with_join_str(format!("{name}.cmd"))
    } else {
        bin_path.with_join_str(name)
    }
}

fn clear_python_entry_points(venv_path: &Path) -> Result<(), Error> {
    let manifest_path
        = venv_path.with_join_str(PYTHON_ENTRY_POINTS_MANIFEST);
    if !manifest_path.fs_exists() {
        return Ok(());
    }

    let bin_path
        = get_venv_bin_path(venv_path);
    for name in manifest_path.fs_read_text()?.lines() {
        if !is_safe_python_entry_point_name(name) {
            continue;
        }

        let path
            = python_entry_point_path(&bin_path, name);
        if path.fs_exists() || path.fs_is_symlink() {
            path.fs_rm()?;
        }
    }

    manifest_path.fs_rm()?;
    Ok(())
}

fn python_entry_point_snippet(
    name: &str,
    site_packages_path: &Path,
    module: &str,
    object: &str,
) -> String {
    let name
        = serde_json::to_string(name).expect("expected valid entry-point name");
    let site_packages_path
        = serde_json::to_string(&site_packages_path.to_file_string()).expect("expected valid site-packages path");
    let module
        = serde_json::to_string(module).expect("expected valid Python module");
    let object
        = serde_json::to_string(object).expect("expected valid Python object");

    format!(
        "import importlib, pathlib, sys\nroot = pathlib.Path({site_packages_path})\nsys.path[:0] = [str(root), *(str(path) for path in root.iterdir() if path.is_dir() and not path.name.startswith('.'))]\nmodule = importlib.import_module({module})\nentry = module\nfor part in {object}.split('.'):\n    entry = getattr(entry, part)\nsys.argv[0] = {name}\nsys.exit(entry())"
    )
}

fn install_python_entry_points(
    binaries: &BTreeMap<String, Binary>,
    venv_path: &Path,
    site_packages_path: &Path,
    target: Option<&PythonTargetEnv>,
    installed_names: &mut BTreeSet<String>,
) -> Result<(), Error> {
    let bin_path
        = get_venv_bin_path(venv_path);
    bin_path.fs_create_dir_all()?;

    let python = find_venv_python_path(venv_path, target)
        .map(|path| path.to_file_string())
        .unwrap_or_else(|| if cfg!(windows) { "python".to_string() } else { "python3".to_string() });

    for (name, binary) in binaries {
        let Binary::Python {module, object} = binary else {
            continue;
        };
        if !is_safe_python_entry_point_name(name) {
            continue;
        }

        let wrapper_path
            = python_entry_point_path(&bin_path, name);
        if !installed_names.contains(name) && (wrapper_path.fs_exists() || wrapper_path.fs_is_symlink()) {
            return Err(Error::PythonPreparation(format!(
                "console script `{name}` conflicts with an existing virtual environment executable",
            )));
        }

        script::make_executable_wrapper(
            &bin_path,
            name,
            &python,
            &[
                "-c".to_string(),
                python_entry_point_snippet(name, site_packages_path, module, object),
            ],
        )?;
        installed_names.insert(name.clone());
    }

    write_python_entry_points_manifest(venv_path, installed_names)?;
    Ok(())
}

fn write_python_entry_points_manifest(venv_path: &Path, names: &BTreeSet<String>) -> Result<(), Error> {
    let contents = if names.is_empty() {
        String::new()
    } else {
        format!("{}\n", names.iter().cloned().collect::<Vec<_>>().join("\n"))
    };

    venv_path
        .with_join_str(PYTHON_ENTRY_POINTS_MANIFEST)
        .fs_write_text(contents)?;
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

fn validate_managed_python_target(locator: &Locator, target: Option<&PythonTargetEnv>) -> Result<(), Error> {
    let Some(target) = target else {
        return Ok(());
    };
    let Reference::Builtin(params) = locator.reference.physical_reference() else {
        return Err(Error::InvalidResolution(format!(
            "Managed Python package {} doesn't use a builtin reference",
            locator.to_print_string(),
        )));
    };
    let managed_minor = format!("{}.{}", params.version.major, params.version.minor);

    if managed_minor != target.python_version {
        return Err(Error::InvalidResolution(format!(
            "Managed Python {} doesn't match the selected Python target {}; constrain @yarnpkg/python to the same major.minor line",
            params.version.to_file_string(),
            target.python_version,
        )));
    }

    Ok(())
}

fn find_venv_python_path(venv_path: &Path, target: Option<&PythonTargetEnv>) -> Option<Path> {
    let bin_path
        = get_venv_bin_path(venv_path);

    let mut candidates = Vec::new();
    if let Some(target) = target {
        candidates.push(format!("python{}", target.python_version));
    }
    candidates.extend(["python".to_string(), "python3".to_string()]);

    candidates.into_iter()
        .map(|candidate| bin_path.with_join_str(candidate))
        .find(|candidate| candidate.fs_exists())
}

fn is_python_project(workspace_path: &Path) -> bool {
    ["pyproject.toml", "setup.py", "setup.cfg"]
        .into_iter()
        .any(|filename| workspace_path.with_join_str(filename).fs_exists())
}

async fn install_workspace_python_project(
    workspace_path: &Path,
    venv_path: &Path,
    site_packages_path: &Path,
    target: Option<&PythonTargetEnv>,
    build_index_url: &str,
    installed_entry_points: &mut BTreeSet<String>,
) -> Result<(), Error> {
    if !is_python_project(workspace_path) {
        return Ok(());
    }

    let python = find_venv_python_path(venv_path, target);
    let wheel = prepare::python::prepare_project(
        workspace_path,
        python.as_ref(),
        target,
        build_index_url,
    ).await?;
    let binaries
        = content_flags::extract_pypi_binaries(&wheel)?;
    let entries = zpm_formats::zip::entries_from_zip(&wheel)?;
    zpm_formats::entries_to_disk(&entries, site_packages_path)?;
    install_python_entry_points(
        &binaries,
        venv_path,
        site_packages_path,
        target,
        installed_entry_points,
    )?;

    Ok(())
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
        = get_venv_bin_path(venv_path);

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
    validate_managed_python_target(locator, target)?;

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
        = prepare::python::find_python_executable_path(&python_home, target)
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

        let (package_locators, workspace_locators)
            = collect_workspace_package_locators(install, &workspace_locator)?;

        let venv_path
            = get_workspace_venv_path(&workspace_path);
        prepare_venv_root(&venv_path)?;
        clear_python_entry_points(&venv_path)?;
        let mut installed_entry_points
            = BTreeSet::new();

        let site_packages_path
            = get_workspace_site_packages_path(&workspace_path, active_fork.target.as_ref());

        remove_inactive_python_lib_dirs(&workspace_path, active_fork.target.as_ref())?;

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

            if let Some(content_flags) = install.install_state.content_flags.get(&package_locator.physical_locator()) {
                install_python_entry_points(
                    &content_flags.binaries,
                    &venv_path,
                    &site_packages_path,
                    active_fork.target.as_ref(),
                    &mut installed_entry_points,
                )?;
            }
        }

        for dependency_locator in workspace_locators {
            let dependency_workspace = project.workspace_by_locator(&dependency_locator)?;
            let build_registry
                = crate::pypi::get_build_registry(&project.config, &dependency_workspace.name)?;
            install_workspace_python_project(
                &dependency_workspace.path,
                &venv_path,
                &site_packages_path,
                active_fork.target.as_ref(),
                &build_registry,
                &mut installed_entry_points,
            ).await?;
        }

        let build_registry
            = crate::pypi::get_build_registry(&project.config, &workspace.name)?;
        install_workspace_python_project(
            &workspace_path,
            &venv_path,
            &site_packages_path,
            active_fork.target.as_ref(),
            &build_registry,
            &mut installed_entry_points,
        ).await?;

        write_python_entry_points_manifest(&venv_path, &installed_entry_points)?;
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
    use zpm_primitives::PythonTargetInput;
    use zpm_utils::System;

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

    #[test]
    fn test_managed_python_must_match_selected_target() {
        let locator = Locator::from_file_string("@yarnpkg/python-darwin-arm64@builtin:3.14.0").unwrap();
        let target = current_target("3.13");

        let error = validate_managed_python_target(&locator, Some(&target)).unwrap_err();

        assert!(error.to_string().contains("doesn't match the selected Python target 3.13"));
    }

    #[test]
    fn test_remove_inactive_python_lib_dirs() {
        let workspace_path = Path::temp_dir_pattern("zpm-venv-test-<>").unwrap();
        let active = workspace_path.with_join_str(".venv/lib/python3.13/site-packages");
        let stale = workspace_path.with_join_str(".venv/lib/python3.11/site-packages");
        active.fs_create_dir_all().unwrap();
        stale.fs_create_dir_all().unwrap();

        remove_inactive_python_lib_dirs(&workspace_path, Some(&current_target("3.13"))).unwrap();

        assert!(active.fs_exists());
        assert!(!stale.fs_exists());
        workspace_path.fs_rm().unwrap();
    }
}
