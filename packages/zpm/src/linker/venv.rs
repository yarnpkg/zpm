//! Venv linker.
//!
//! Each workspace of a venv island gets a regular virtual environment in
//! `<workspace>/.venv`, usable by any tool without going through Yarn:
//!
//! ```text
//! .venv/
//!   pyvenv.cfg                  home = <interpreter dir>
//!   bin/python -> <interpreter>
//!   bin/python3, bin/python3.X  (symlinks to python)
//!   bin/<console scripts>       generated from entry_points.txt
//!   lib/python3.X/site-packages/
//!     <wheel contents>          cloned or hardlinked from a shared store
//!     _yarn_<workspace>.pth     editable installs of local workspaces
//! ```
//!
//! Wheels are unpacked once in `<globalFolder>/pypi-unpacked/<key>` and
//! then cloned (copy-on-write, macOS) or hardlinked into the venvs, so N
//! venvs don't cost N times the disk. A venv is only rebuilt when its
//! content changes: `.venv/.yarn-state` stores a hash of what it contains.

use std::{collections::{BTreeMap, BTreeSet}, os::unix::fs::PermissionsExt};

use rayon::prelude::*;

use zpm_primitives::{Ident, Locator, Range, Reference};
use zpm_utils::{Hash64, Path, ToFileString, ToHumanString};

use crate::{
    build::BuildRequests,
    error::Error,
    fetchers::PackageData,
    install::Install,
    linker::LinkResult,
    project::Project,
    python_env::{PythonEnv, PythonTargets, evaluate_marker_str},
    python_interpreter::{Interpreter, ensure_interpreter},
};

fn edge_applies(range: &Range, env: &PythonEnv) -> bool {
    let Range::PypiSpecifier(params) = range else {
        return true;
    };

    params.parameters.as_ref()
        .and_then(|parameters| parameters.marker.as_deref())
        .map_or(true, |marker| evaluate_marker_str(marker, env))
}

fn physical_range(range: &Range) -> &Range {
    match range {
        Range::Virtual(params) => physical_range(&params.inner),
        range => range,
    }
}

/// The packages to install in a workspace's venv: its transitive
/// dependencies, minus the edges whose markers don't apply here. Local
/// workspaces are returned separately, as they're installed in editable
/// mode rather than copied.
struct VenvPackages {
    packages: BTreeMap<Ident, Locator>,
    workspaces: BTreeSet<Locator>,
}

/// Walks the island's own resolution maps (the project-wide tree merges
/// every island, and two islands may resolve the same descriptor to
/// different versions).
fn collect_venv_packages(install: &Install, island_id: &str, workspace_locator: &Locator, env: &PythonEnv) -> Result<VenvPackages, Error> {
    let empty_d2l
        = BTreeMap::new();
    let empty_resolutions
        = BTreeMap::new();

    let descriptor_to_locator
        = install.install_state.island_descriptor_to_locator.get(island_id).unwrap_or(&empty_d2l);
    let resolutions
        = install.install_state.island_normalized_resolutions.get(island_id).unwrap_or(&empty_resolutions);

    let system
        = zpm_utils::System::current();

    let mut packages
        = BTreeMap::<Ident, Locator>::new();
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

        let Some(resolution) = resolutions.get(&locator) else {
            continue;
        };

        for descriptor in resolution.dependencies.values() {
            if !edge_applies(physical_range(&descriptor.range), env) {
                continue;
            }

            let Some(mut dependency_locator) = descriptor_to_locator.get(descriptor).cloned() else {
                continue;
            };

            // Packages with per-platform artifacts: pick the current variant
            if let Some(dependency_resolution) = resolutions.get(&dependency_locator) {
                if !dependency_resolution.variants.is_empty() {
                    let variant = dependency_resolution.variants.iter()
                        .filter_map(|variant| descriptor_to_locator.get(variant))
                        .find(|variant_locator| resolutions.get(variant_locator).map_or(false, |variant_resolution| variant_resolution.requirements.validate_system(system)))
                        .cloned();

                    // No artifact for this platform: the edge can only be
                    // guarded by a marker we couldn't evaluate (pywin32 is
                    // required behind `sys_platform == 'win32'`), so it
                    // doesn't apply to this venv
                    let Some(variant) = variant else {
                        continue;
                    };

                    dependency_locator = variant;
                }
            }

            queue.push(dependency_locator.clone());

            let physical_locator
                = dependency_locator.physical_locator();

            if physical_locator.reference.is_workspace_reference() {
                workspaces.insert(physical_locator);
                continue;
            }

            if let Some(existing_locator) = packages.get(&physical_locator.ident) {
                if existing_locator != &physical_locator {
                    return Err(Error::InvalidResolution(format!(
                        "Multiple versions of {} are required in the same venv ({} and {})",
                        physical_locator.ident.as_str(),
                        existing_locator.to_print_string(),
                        physical_locator.to_print_string(),
                    )));
                }

                continue;
            }

            packages.insert(physical_locator.ident.clone(), physical_locator);
        }
    }

    workspaces.remove(workspace_locator);

    Ok(VenvPackages {packages, workspaces})
}

fn is_pypi_locator(locator: &Locator) -> bool {
    matches!(locator.reference.physical_reference(), Reference::PypiRegistry(_) | Reference::PypiShorthand(_))
}

/// Places a package holding a plain Python module (published as an npm
/// archive, or a local folder) in site-packages, under its name.
fn link_module_package(site_packages: &Path, locator: &Locator, package_data: &PackageData) -> Result<(), Error> {
    let package_path
        = site_packages.with_join_str(locator.ident.as_str());

    if package_path.fs_symlink_metadata().is_ok() {
        package_path.fs_rm()?;
    }

    package_path.fs_create_parent()?;

    match package_data {
        PackageData::Local {package_directory, ..} => {
            package_path.fs_symlink(package_directory)?;
        },

        PackageData::Zip {..} => {
            package_path.fs_create_dir_all()?;
            crate::linker::helpers::fs_extract_archive(&package_path, package_data)?;
        },

        PackageData::MissingZip {..} | PackageData::Abstract => {},
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Shared unpacked store
// ---------------------------------------------------------------------------

fn store_root(project: &Project) -> Path {
    project.config.settings.global_folder.value
        .with_join_str("pypi-unpacked")
}

/// Unpacks a wheel in the shared store (once per wheel content) and returns
/// the path of the unpacked folder.
fn ensure_unpacked(project: &Project, interpreter: &Interpreter, locator: &Locator, package_data: &PackageData) -> Result<Path, Error> {
    let PackageData::Zip {archive_path, checksum, ..} = package_data else {
        return Err(Error::InvalidResolution(format!("Expected a wheel archive for {}", locator.to_print_string())));
    };

    // Released artifacts are immutable, so their locator (which includes the
    // version, and the artifact URL for platform variants) is a stable key.
    // The checksum isn't: it's only known once the archive is first hashed.
    let _ = checksum;

    let key
        = Hash64::from_data(locator.to_file_string().as_bytes());

    // Source distributions are built into a wheel first (once per machine)
    let is_sdist = {
        let archive_path = archive_path.to_file_string();
        archive_path.ends_with(".tar.gz") || archive_path.ends_with(".src.zip")
    };

    let archive_path = if is_sdist {
        crate::python_build::build_wheel_from_sdist(project, interpreter, locator, archive_path, &key)?
    } else {
        archive_path.clone()
    };
    let archive_path = &archive_path;

    let root
        = store_root(project);

    let entry_path
        = root.with_join_str(format!("{}-{}", locator.ident.slug(), key.short()));

    if entry_path.with_join_str(".complete").fs_exists() {
        return Ok(entry_path);
    }

    let tmp_path
        = root.with_join_str(format!(".tmp-{}-{}-{}", std::process::id(), locator.ident.slug(), key.short()));

    let _ = tmp_path.fs_rm();
    tmp_path.fs_create_dir_all()?;

    let bytes
        = archive_path.fs_read()?;

    for entry in zpm_formats::zip::entries_from_zip(&bytes)? {
        let name
            = entry.name.as_str();

        if name.ends_with('/') || name.split('/').any(|segment| segment == "..") {
            continue;
        }

        let target
            = tmp_path.with_join_str(name);

        target.fs_create_parent()?;
        target.fs_write(&entry.data)?;

        let mode
            = if entry.mode & 0o111 != 0 { 0o755 } else { 0o644 };

        target.fs_set_permissions(std::fs::Permissions::from_mode(mode))?;
    }

    tmp_path.with_join_str(".complete").fs_write([])?;

    if tmp_path.fs_rename(&entry_path).is_err() {
        let _ = tmp_path.fs_rm();

        if !entry_path.with_join_str(".complete").fs_exists() {
            return Err(Error::InvalidResolution(format!("Failed to unpack {}", locator.to_print_string())));
        }
    }

    Ok(entry_path)
}

fn list_files(root: &Path, rel: &str, out: &mut Vec<String>) -> Result<(), Error> {
    for entry in root.with_join_str(rel).fs_read_dir()?.flatten() {
        let name
            = entry.file_name().to_string_lossy().to_string();

        let child
            = if rel.is_empty() { name } else { format!("{}/{}", rel, name) };

        if entry.file_type().map(|file_type| file_type.is_dir()).unwrap_or(false) {
            list_files(root, &child, out)?;
        } else {
            out.push(child);
        }
    }

    Ok(())
}

/// Copies the content of a store entry into the venv. On copy-on-write
/// filesystems each top-level entry (package folder, dist-info, ...) is
/// cloned in one `clonefile` call; elsewhere files are hardlinked one by
/// one. `.data/` folders are relocated according to the wheel spec.
fn materialize(store_entry: &Path, site_packages: &Path, venv: &Path, use_clone: bool) -> Result<(), Error> {
    for entry in store_entry.fs_read_dir()?.flatten() {
        let name
            = entry.file_name().to_string_lossy().to_string();

        if name == ".complete" {
            continue;
        }

        let source
            = store_entry.with_join_str(&name);

        if name.ends_with(".data") && entry.path().is_dir() {
            for scheme in source.fs_read_dir()?.flatten() {
                let scheme_name
                    = scheme.file_name().to_string_lossy().to_string();

                let destination = match scheme_name.as_str() {
                    "purelib" | "platlib" => site_packages.clone(),
                    "scripts" => venv.with_join_str("bin"),
                    "data" => venv.clone(),
                    "headers" => venv.with_join_str("include"),
                    _ => continue,
                };

                materialize_tree(&source.with_join_str(&scheme_name), &destination, use_clone)?;
            }

            continue;
        }

        let target
            = site_packages.with_join_str(&name);

        if use_clone && !target.fs_exists() && source.fs_clonefile(&target).is_ok() {
            continue;
        }

        if entry.path().is_dir() {
            materialize_tree(&source, &target, use_clone)?;
        } else {
            link_file(&source, &target, use_clone)?;
        }
    }

    Ok(())
}

fn link_file(source: &Path, target: &Path, use_clone: bool) -> Result<(), Error> {
    target.fs_create_parent()?;

    if target.fs_symlink_metadata().is_ok() {
        target.fs_rm_file()?;
    }

    let linked = if use_clone {
        source.fs_clonefile(target).is_ok()
    } else {
        std::fs::hard_link(source.to_path_buf(), target.to_path_buf()).is_ok()
    };

    if !linked {
        source.fs_copy_file(target)?;
    }

    Ok(())
}

/// Merges the content of `source` into `destination` (both folders).
fn materialize_tree(source: &Path, destination: &Path, use_clone: bool) -> Result<(), Error> {
    let mut files
        = Vec::new();

    list_files(source, "", &mut files)?;

    for rel in files {
        link_file(&source.with_join_str(&rel), &destination.with_join_str(&rel), use_clone)?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Venv layout
// ---------------------------------------------------------------------------

pub fn python_dir(interpreter: &Interpreter) -> String {
    format!("python{}.{}", interpreter.version.major, interpreter.version.minor)
}

pub fn site_packages_path(venv: &Path, python_dir: &str) -> Path {
    venv.with_join_str("lib")
        .with_join_str(python_dir)
        .with_join_str("site-packages")
}

pub fn workspace_venv_path(workspace_path: &Path) -> Path {
    workspace_path.with_join_str(".venv")
}

fn write_venv_skeleton(venv: &Path, interpreter: &Interpreter) -> Result<(), Error> {
    let bin
        = venv.with_join_str("bin");

    bin.fs_create_dir_all()?;

    venv.with_join_str("pyvenv.cfg").fs_write(format!(
        "home = {}\ninclude-system-site-packages = false\nversion = {}\nexecutable = {}\ncommand = yarn install\n",
        interpreter.home.to_file_string(),
        interpreter.version.full(),
        interpreter.executable.to_file_string(),
    ))?;

    let python
        = bin.with_join_str("python");

    python.fs_symlink(&interpreter.executable)?;

    for alias in [format!("python{}", interpreter.version.major), format!("python{}.{}", interpreter.version.major, interpreter.version.minor)] {
        std::os::unix::fs::symlink("python", bin.with_join_str(&alias).to_path_buf())
            .map_err(zpm_utils::PathError::from)?;
    }

    let activate
        = format!(
            "# Generated by Yarn; source this file to activate the venv\ndeactivate () {{\n    if [ -n \"${{_OLD_VIRTUAL_PATH:-}}\" ]; then PATH=\"$_OLD_VIRTUAL_PATH\"; export PATH; unset _OLD_VIRTUAL_PATH; fi\n    unset VIRTUAL_ENV\n    if [ ! \"${{1:-}}\" = \"nondestructive\" ]; then unset -f deactivate; fi\n}}\ndeactivate nondestructive\nVIRTUAL_ENV=\"{}\"\nexport VIRTUAL_ENV\n_OLD_VIRTUAL_PATH=\"$PATH\"\nPATH=\"$VIRTUAL_ENV/bin:$PATH\"\nexport PATH\nhash -r 2>/dev/null || true\n",
            venv.to_file_string(),
        );

    bin.with_join_str("activate").fs_write(activate)?;

    venv.with_join_str(".gitignore").fs_write("*\n")?;

    Ok(())
}

fn write_console_script(venv: &Path, name: &str, module: &str, object: &str) -> Result<(), Error> {
    if name.is_empty() || name.contains('/') {
        return Ok(());
    }

    let script
        = venv.with_join_str("bin").with_join_str(name);

    let python
        = venv.with_join_str("bin/python");

    let import_name
        = object.split('.').next().unwrap_or(object);

    let content
        = format!(
            "#!{}\n# -*- coding: utf-8 -*-\nimport re\nimport sys\nfrom {} import {}\nif __name__ == \"__main__\":\n    sys.argv[0] = re.sub(r\"(-script\\.pyw|\\.exe)?$\", \"\", sys.argv[0])\n    sys.exit({}())\n",
            python.to_file_string(),
            module,
            import_name,
            object,
        );

    if script.fs_symlink_metadata().is_ok() {
        script.fs_rm_file()?;
    }

    script.fs_write(content)?;
    script.fs_set_permissions(std::fs::Permissions::from_mode(0o755))?;

    Ok(())
}

/// Parses `entry_points.txt` and returns the console scripts.
fn console_scripts(text: &str) -> Vec<(String, String, String)> {
    let mut in_section
        = false;

    let mut scripts
        = Vec::new();

    for line in text.lines() {
        let line
            = line.trim();

        if line.starts_with('[') {
            in_section = line == "[console_scripts]" || line == "[gui_scripts]";
            continue;
        }

        if !in_section || line.is_empty() || line.starts_with('#') {
            continue;
        }

        let Some((name, target)) = line.split_once('=') else {
            continue;
        };

        let target
            = target.split('[').next().unwrap_or(target).trim();

        let Some((module, object)) = target.split_once(':') else {
            continue;
        };

        scripts.push((name.trim().to_string(), module.trim().to_string(), object.trim().to_string()));
    }

    scripts
}

fn install_console_scripts(venv: &Path, store_entry: &Path) -> Result<(), Error> {
    for entry in store_entry.fs_read_dir()?.flatten() {
        let name
            = entry.file_name().to_string_lossy().to_string();

        if !name.ends_with(".dist-info") {
            continue;
        }

        let Ok(text) = store_entry.with_join_str(&name).with_join_str("entry_points.txt").fs_read_text() else {
            continue;
        };

        for (script, module, object) in console_scripts(&text) {
            write_console_script(venv, &script, &module, &object)?;
        }
    }

    Ok(())
}

/// The folder to add to `sys.path` for an editable workspace: `src/` when
/// the project uses the src layout, the workspace root otherwise.
pub fn editable_root(workspace_path: &Path) -> Path {
    let src
        = workspace_path.with_join_str("src");

    let uses_src_layout
        = src.fs_read_dir()
            .map(|entries| entries.flatten().any(|entry| entry.path().join("__init__.py").is_file()))
            .unwrap_or(false);

    if uses_src_layout {
        return src;
    }

    workspace_path.clone()
}

/// Writes the console scripts declared by a workspace's pyproject.toml
/// (`[project.scripts]`).
fn install_workspace_scripts(venv: &Path, workspace_path: &Path) -> Result<(), Error> {
    let Ok(text) = workspace_path.with_join_str("pyproject.toml").fs_read_text() else {
        return Ok(());
    };

    let Ok(document) = text.parse::<toml_edit::DocumentMut>() else {
        return Ok(());
    };

    let Some(scripts) = document.get("project").and_then(|project| project.get("scripts")).and_then(|scripts| scripts.as_table_like()) else {
        return Ok(());
    };

    for (name, target) in scripts.iter() {
        let Some(target) = target.as_str() else {
            continue;
        };

        let target
            = target.split('[').next().unwrap_or(target).trim();

        let Some((module, object)) = target.split_once(':') else {
            continue;
        };

        write_console_script(venv, name, module.trim(), object.trim())?;
    }

    Ok(())
}

fn venv_state(interpreter: &Interpreter, packages: &[(Locator, Option<Hash64>)], editables: &[Path]) -> String {
    let mut parts
        = vec![format!("v1:{}:{}", interpreter.executable.to_file_string(), interpreter.version.full())];

    for (locator, checksum) in packages {
        // Checksums may only be known after the first install; locators of
        // released artifacts are immutable, so they're enough
        let _ = checksum;
        parts.push(locator.to_file_string());
    }

    for editable in editables {
        parts.push(editable.to_file_string());
    }

    parts.join("\n")
}

pub fn island_python_version(project: &Project, island_id: &str) -> String {
    project.config.settings.unstable_islands.get(island_id)
        .and_then(|definition| definition.python_version.value.clone())
        .unwrap_or_else(|| project.config.settings.python_version.value.clone())
}

pub async fn link_island_venv(project: &Project, install: &Install, island: &crate::island::ResolvedIsland) -> Result<LinkResult, Error> {
    let mut packages_by_location
        = BTreeMap::new();

    let python_version
        = island_python_version(project, &island.id);

    let targets
        = PythonTargets::from_config(&project.config, Some(&python_version));

    let env
        = targets.current_env();

    let interpreter
        = ensure_interpreter(&project.config, &project.http_client, &python_version).await?;

    let use_clone
        = crate::linker::helpers::clonefile_supported(&store_root(project), &project.project_cwd);

    for workspace_ident in &island.workspace_idents {
        let workspace
            = project.workspace_by_ident(workspace_ident)?;

        let workspace_locator
            = workspace.locator();

        packages_by_location.insert(workspace.rel_path.clone(), workspace_locator.clone());

        let venv
            = workspace_venv_path(&workspace.path);

        let site_packages
            = site_packages_path(&venv, &python_dir(&interpreter));

        let collected
            = collect_venv_packages(install, &island.id, &workspace_locator, &env)?;

        let package_entries
            = collected.packages.values()
                .map(|locator| (locator.clone(), install.package_data.get(locator).and_then(|data| data.checksum())))
                .collect::<Vec<_>>();

        let mut editable_workspaces
            = vec![];

        if workspace.path.with_join_str("pyproject.toml").fs_exists() {
            editable_workspaces.push(workspace.path.clone());
        }

        for dependency in &collected.workspaces {
            if let Ok(dependency_workspace) = project.workspace_by_locator(dependency) {
                if dependency_workspace.path.with_join_str("pyproject.toml").fs_exists() {
                    editable_workspaces.push(dependency_workspace.path.clone());
                }
            }
        }

        let editables
            = editable_workspaces.iter()
                .map(editable_root)
                .collect::<Vec<_>>();

        for (locator, _) in &package_entries {
            packages_by_location.insert(site_packages.relative_to(&project.project_cwd).with_join_str(locator.ident.as_str()), locator.clone());
        }

        let state
            = venv_state(&interpreter, &package_entries, &editables);

        let state_path
            = venv.with_join_str(".yarn-state");

        let is_up_to_date
            = state_path.fs_read_text().ok().as_deref() == Some(state.as_str())
                && venv.with_join_str("bin/python").fs_exists();

        if is_up_to_date {
            continue;
        }

        if venv.fs_symlink_metadata().is_ok() {
            venv.fs_rm()?;
        }

        site_packages.fs_create_dir_all()?;
        write_venv_skeleton(&venv, &interpreter)?;

        // Unpacking (and building sdists) is independent per package, and
        // each package only writes its own files in site-packages
        package_entries.par_iter()
            .map(|(locator, _)| -> Result<(), Error> {
                let Some(package_data) = install.package_data.get(locator) else {
                    return Ok(());
                };

                // Non-PyPI packages (npm-style archives, folders, links)
                // hold a Python module as-is; they're placed in
                // site-packages under their name
                if !is_pypi_locator(locator) {
                    return link_module_package(&site_packages, locator, package_data);
                }

                if let PackageData::Zip {..} = package_data {
                    let store_entry
                        = ensure_unpacked(project, &interpreter, locator, package_data)?;

                    materialize(&store_entry, &site_packages, &venv, use_clone)?;
                    install_console_scripts(&venv, &store_entry)?;
                }

                Ok(())
            })
            .collect::<Result<Vec<_>, _>>()?;

        if !editables.is_empty() {
            let pth
                = editables.iter()
                    .map(|path| path.to_file_string())
                    .collect::<Vec<_>>()
                    .join("\n");

            site_packages.with_join_str(format!("_yarn_{}.pth", workspace.name.slug())).fs_write(format!("{}\n", pth))?;
        }

        for workspace_path in &editable_workspaces {
            install_workspace_scripts(&venv, workspace_path)?;
        }

        state_path.fs_write(&state)?;
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

    #[test]
    fn parses_console_scripts() {
        let scripts
            = console_scripts("[console_scripts]\nfoo = foo.cli:main\nbar = bar:app.run [extra]\n[other]\nbaz = baz:x\n");

        assert_eq!(scripts, vec![
            ("foo".to_string(), "foo.cli".to_string(), "main".to_string()),
            ("bar".to_string(), "bar".to_string(), "app.run".to_string()),
        ]);
    }
}
