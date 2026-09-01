use std::process::{Command, Output};

use zpm_primitives::{Ident, PypiVersion, PythonTargetEnv, canonicalize_pypi_name};
use zpm_utils::{FromFileString, Path, ToFileString};

use crate::error::Error;
use crate::fetchers::PackageData;

const PEP517_RUNNER: &str = r#"
import importlib
import os
import pathlib
import subprocess
import sys
import venv


def fail(message):
    print(message, file=sys.stderr)
    raise SystemExit(1)


def find_project_root(extraction_root):
    markers = ("pyproject.toml", "setup.py", "setup.cfg")
    if any((extraction_root / marker).is_file() for marker in markers):
        return extraction_root

    children = [child for child in extraction_root.iterdir() if child.is_dir()]
    candidates = [
        child for child in children
        if any((child / marker).is_file() for marker in markers)
    ]
    if len(candidates) != 1:
        fail("sdist must contain exactly one Python project root")
    return candidates[0]


def read_build_system(project_root):
    pyproject_path = project_root / "pyproject.toml"
    if not pyproject_path.is_file():
        return {
            "requires": ["setuptools>=40.8.0"],
            "build-backend": "setuptools.build_meta:__legacy__",
            "backend-path": [],
        }

    try:
        import tomllib
    except ImportError:
        fail("building sdists with pyproject.toml requires Python 3.11 or newer")

    with pyproject_path.open("rb") as stream:
        document = tomllib.load(stream)

    build_system = document.get("build-system")
    if build_system is None:
        return {
            "requires": ["setuptools>=40.8.0"],
            "build-backend": "setuptools.build_meta:__legacy__",
            "backend-path": [],
        }
    if not isinstance(build_system, dict):
        fail("pyproject.toml [build-system] must be a table")

    requires = build_system.get("requires")
    backend = build_system.get("build-backend", "setuptools.build_meta:__legacy__")
    backend_path = build_system.get("backend-path", [])
    if not isinstance(requires, list) or not all(isinstance(item, str) for item in requires):
        fail("build-system.requires must be an array of strings")
    if not isinstance(backend, str) or not backend:
        fail("build-system.build-backend must be a non-empty string")
    if not isinstance(backend_path, list) or not all(isinstance(item, str) for item in backend_path):
        fail("build-system.backend-path must be an array of strings")

    return {
        "requires": requires,
        "build-backend": backend,
        "backend-path": backend_path,
    }


def build_environment_python(work_root, requirements):
    environment_root = work_root / "build-environment"
    venv.EnvBuilder(with_pip=True, clear=True).create(environment_root)
    if os.name == "nt":
        python = environment_root / "Scripts" / "python.exe"
    else:
        python = environment_root / "bin" / "python"

    if requirements:
        subprocess.run([
            str(python), "-m", "pip", "install",
            "--disable-pip-version-check", "--no-input",
            *requirements,
        ], check=True)
    return python


def load_backend(project_root, build_system):
    backend_paths = []
    for raw_path in build_system["backend-path"]:
        candidate = (project_root / raw_path).resolve()
        try:
            candidate.relative_to(project_root.resolve())
        except ValueError:
            fail("build-system.backend-path entries must stay within the source tree")
        backend_paths.append(str(candidate))

    sys.path[:0] = backend_paths
    os.chdir(project_root)

    module_name, separator, object_path = build_system["build-backend"].partition(":")
    backend = importlib.import_module(module_name)
    if separator:
        for component in object_path.split("."):
            backend = getattr(backend, component)

    return backend


def select_build_hook(backend, requested_hook):
    hook = getattr(backend, requested_hook, None)
    selected_hook = requested_hook
    if hook is None and requested_hook == "build_editable":
        selected_hook = "build_wheel"
        hook = getattr(backend, selected_hook, None)
    if hook is None:
        fail(f"PEP 517 backend has no {requested_hook} hook")
    return selected_hook, hook


def install_dynamic_build_requirements(project_root, build_system, requested_hook):
    backend = load_backend(project_root, build_system)
    selected_hook, _ = select_build_hook(backend, requested_hook)
    requirements_hook = getattr(backend, f"get_requires_for_{selected_hook}", None)
    if requirements_hook is None:
        return

    requirements = requirements_hook({})
    if not isinstance(requirements, list) or not all(isinstance(item, str) for item in requirements):
        fail(f"PEP 517 get_requires_for_{selected_hook} must return an array of strings")
    if requirements:
        subprocess.run([
            sys.executable, "-m", "pip", "install",
            "--disable-pip-version-check", "--no-input",
            *requirements,
        ], check=True)


def run_hook(project_root, output_root, build_system, requested_hook):
    backend = load_backend(project_root, build_system)

    _, hook = select_build_hook(backend, requested_hook)

    wheel_name = hook(str(output_root), {}, None)
    if not isinstance(wheel_name, str):
        fail("PEP 517 build_wheel must return a wheel filename")

    wheel_path = (output_root / wheel_name).resolve()
    try:
        wheel_path.relative_to(output_root.resolve())
    except ValueError:
        fail("PEP 517 backend returned a wheel outside the output directory")
    if wheel_path.name != wheel_name or not wheel_name.endswith(".whl"):
        fail("PEP 517 backend returned an invalid wheel filename")
    if not wheel_path.is_file():
        fail(f"PEP 517 backend did not create {wheel_name}")

    print("ZPM_WHEEL=" + wheel_name)


def main():
    mode, requested_hook, extraction_root, output_root, work_root = sys.argv[1:]
    extraction_root = pathlib.Path(extraction_root).resolve()
    output_root = pathlib.Path(output_root).resolve()
    work_root = pathlib.Path(work_root).resolve()
    project_root = find_project_root(extraction_root)
    build_system = read_build_system(project_root)

    if mode == "prepare":
        python = build_environment_python(work_root, build_system["requires"])
        build_env = os.environ.copy()
        build_env["PATH"] = str(python.parent) + os.pathsep + build_env.get("PATH", "")
        subprocess.run([
            str(python), __file__, "requirements", requested_hook,
            str(extraction_root), str(output_root), str(work_root),
        ], check=True, env=build_env)
        subprocess.run([
            str(python), __file__, "hook", requested_hook,
            str(extraction_root), str(output_root), str(work_root),
        ], check=True, env=build_env)
    elif mode == "requirements":
        install_dynamic_build_requirements(project_root, build_system, requested_hook)
    elif mode == "hook":
        run_hook(project_root, output_root, build_system, requested_hook)
    else:
        fail(f"unknown runner mode: {mode}")


if __name__ == "__main__":
    main()
"#;

fn invalid_sdist(filename: &str, message: impl AsRef<str>) -> Error {
    Error::PythonPreparation(format!("`{filename}`: {}", message.as_ref()))
}

fn validate_archive_entries(filename: &str, entries: &[zpm_formats::Entry<'_>]) -> Result<(), Error> {
    for entry in entries {
        if !entry.name.is_forward() || entry.name.segments().any(|segment| segment == "..") {
            return Err(invalid_sdist(filename, format!(
                "archive entry `{}` escapes the source directory",
                entry.name.to_file_string(),
            )));
        }
    }

    Ok(())
}

fn unpack_sdist(source: &[u8], filename: &str, extraction_root: &Path) -> Result<(), Error> {
    if filename.ends_with(".tar.gz") || filename.ends_with(".tgz") {
        let tar = zpm_formats::tar::unpack_tgz(source)?;
        let entries = zpm_formats::tar::entries_from_tar(&tar)?;
        validate_archive_entries(filename, &entries)?;
        zpm_formats::entries_to_disk(&entries, extraction_root)?;
    } else if filename.ends_with(".zip") {
        let entries = zpm_formats::zip::entries_from_zip(source)?;
        validate_archive_entries(filename, &entries)?;
        zpm_formats::entries_to_disk(&entries, extraction_root)?;
    } else {
        return Err(invalid_sdist(filename, "only .tar.gz, .tgz, and .zip archives are supported"));
    }

    Ok(())
}

fn python_candidates(target: Option<&PythonTargetEnv>) -> Vec<String> {
    if let Ok(python) = std::env::var("ZPM_PYTHON_EXECUTABLE") {
        return vec![python];
    }

    let mut candidates = Vec::new();
    if let Some(target) = target {
        if target.implementation_name.as_deref() == Some("pypy") {
            candidates.push(format!("pypy{}", target.python_version));
            candidates.push("pypy3".to_string());
        } else {
            candidates.push(format!("python{}", target.python_version));
        }
    }

    if cfg!(windows) {
        candidates.extend(["python".to_string(), "python3".to_string()]);
    } else {
        candidates.extend(["python3".to_string(), "python".to_string()]);
    }

    candidates.dedup();
    candidates
}

fn python_matches_target(python: &str, target: &PythonTargetEnv) -> Result<bool, std::io::Error> {
    let output = Command::new(python)
        .args([
            "-c",
            "import platform,sys;print(f'{sys.version_info.major}.{sys.version_info.minor}');print(sys.implementation.name);print(sys.platform);print(platform.machine())",
        ])
        .output()?;

    if !output.status.success() {
        return Ok(false);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut fields = stdout.lines();
    let version = fields.next().unwrap_or_default();
    let implementation = fields.next().unwrap_or_default();
    let sys_platform = fields.next().unwrap_or_default();
    let platform_machine = fields.next().unwrap_or_default();

    Ok(version == target.python_version
        && target.implementation_name.as_deref().map_or(true, |expected| expected == implementation)
        && target.sys_platform.as_deref().map_or(true, |expected| expected == sys_platform)
        && target.platform_machine.as_deref().map_or(true, |expected| expected == platform_machine))
}

pub fn find_python_executable_path(python_home: &Path, target: Option<&PythonTargetEnv>) -> Option<Path> {
    let bin_path = if cfg!(windows) {
        python_home.with_join_str("Scripts")
    } else {
        python_home.with_join_str("bin")
    };
    let suffix = if cfg!(windows) { ".exe" } else { "" };
    let mut candidates = Vec::new();

    if let Some(target) = target {
        candidates.push(format!("python{}{}", target.python_version, suffix));
    }
    candidates.extend([
        format!("python{suffix}"),
        format!("python3{suffix}"),
    ]);

    candidates.into_iter()
        .map(|candidate| bin_path.with_join_str(candidate))
        .find(|candidate| candidate.fs_exists())
}

fn run_pep517_runner(
    runner: &Path,
    extraction_root: &Path,
    output_root: &Path,
    work_root: &Path,
    requested_hook: &str,
    preferred_python: Option<&Path>,
    target: Option<&PythonTargetEnv>,
) -> Result<Output, Error> {
    let mut last_candidate = None;

    let candidates = preferred_python
        .map(|python| vec![python.to_file_string()])
        .unwrap_or_else(|| python_candidates(target));

    for python in candidates {
        last_candidate = Some(python.clone());
        if let Some(target) = target {
            match python_matches_target(&python, target) {
                Ok(true) => {},
                Ok(false) => continue,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    continue;
                },
                Err(error) => return Err(error.into()),
            }
        }

        let output = Command::new(&python)
            .arg(runner.to_path_buf())
            .arg("prepare")
            .arg(requested_hook)
            .arg(extraction_root.to_path_buf())
            .arg(output_root.to_path_buf())
            .arg(work_root.to_path_buf())
            .output();

        match output {
            Ok(output) => return Ok(output),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {},
            Err(error) => return Err(error.into()),
        }
    }

    let target_hint = target
        .map(|target| format!(" matching Python {}", target.python_version))
        .unwrap_or_default();
    Err(Error::PythonPreparation(format!(
        "unable to find a Python interpreter{target_hint} (last candidate: {})",
        last_candidate.unwrap_or_default(),
    )))
}

fn wheel_name_from_output(output: &Output, filename: &str) -> Result<String, Error> {
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let detail = if stderr.trim().is_empty() {
            "PEP 517 backend failed"
        } else {
            stderr.trim()
        };
        return Err(invalid_sdist(filename, detail));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout.lines()
        .rev()
        .find_map(|line| line.strip_prefix("ZPM_WHEEL="))
        .map(|name| name.to_string())
        .ok_or_else(|| invalid_sdist(filename, "PEP 517 backend did not report its wheel"))
}

fn metadata_field<'a>(metadata: &'a str, field: &str) -> Option<&'a str> {
    metadata.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.eq_ignore_ascii_case(field).then(|| value.trim())
    })
}

fn validate_wheel(
    wheel: &[u8],
    wheel_filename: &str,
    expected_ident: &Ident,
    expected_version: &PypiVersion,
    target: Option<&PythonTargetEnv>,
    sdist_filename: &str,
) -> Result<(), Error> {
    let entries = zpm_formats::zip::entries_from_zip(wheel)
        .map_err(|error| invalid_sdist(sdist_filename, format!("backend produced an invalid wheel: {error}")))?;
    let metadata_entry = entries.iter()
        .find(|entry| entry.name.as_str().ends_with(".dist-info/METADATA"))
        .ok_or_else(|| invalid_sdist(sdist_filename, "backend produced a wheel without .dist-info/METADATA"))?;
    let metadata = std::str::from_utf8(&metadata_entry.data)
        .map_err(|_| invalid_sdist(sdist_filename, "wheel METADATA is not UTF-8"))?;

    let name = metadata_field(metadata, "Name")
        .ok_or_else(|| invalid_sdist(sdist_filename, "wheel METADATA has no Name field"))?;
    if canonicalize_pypi_name(name) != expected_ident.as_str() {
        return Err(invalid_sdist(sdist_filename, format!(
            "backend produced package `{name}`, expected `{}`",
            expected_ident.to_file_string(),
        )));
    }

    let version = metadata_field(metadata, "Version")
        .ok_or_else(|| invalid_sdist(sdist_filename, "wheel METADATA has no Version field"))?;
    let version = PypiVersion::from_file_string(version)
        .map_err(|_| invalid_sdist(sdist_filename, format!("wheel has invalid version `{version}`")))?;
    if !version.cmp_pep440(expected_version)
        .map_err(|error| invalid_sdist(sdist_filename, error.to_string()))?
        .is_eq()
    {
        return Err(invalid_sdist(sdist_filename, format!(
            "backend produced version `{}`, expected `{}`",
            version.to_file_string(),
            expected_version.to_file_string(),
        )));
    }

    if let Some(target) = target {
        let distribution = crate::pypi::PypiDistribution {
            filename: wheel_filename.to_string(),
            packagetype: "bdist_wheel".to_string(),
            url: String::new(),
            upload_time: None,
            upload_time_iso_8601: None,
            requires_python: None,
        };
        if crate::pypi::select_best_wheel(&[distribution], Some(target)).is_none() {
            return Err(invalid_sdist(sdist_filename, format!(
                "backend produced wheel `{wheel_filename}`, which is incompatible with the selected Python target",
            )));
        }
    }

    Ok(())
}

/// Builds a wheel from a Python source distribution using its PEP 517 backend.
///
/// Artifact selection, downloading, and caching intentionally remain owned by
/// the PyPI fetcher. This function only turns downloaded source bytes into a
/// validated wheel.
pub async fn prepare_sdist(
    source: &[u8],
    filename: &str,
    expected_ident: &Ident,
    expected_version: &PypiVersion,
    target: Option<&PythonTargetEnv>,
    managed_python: Option<&PackageData>,
) -> Result<Vec<u8>, Error> {
    let work_root = Path::temp_dir_pattern("zpm-python-sdist-<>")?;
    let extraction_root = work_root.with_join_str("source");
    let output_root = work_root.with_join_str("wheel");
    let runner = work_root.with_join_str("pep517_runner.py");

    let result = async {
        extraction_root.fs_create_dir_all()?;
        output_root.fs_create_dir_all()?;
        unpack_sdist(source, filename, &extraction_root)?;
        runner.fs_write_text(PEP517_RUNNER)?;

        let preferred_python = if let Some(managed_python) = managed_python {
            let python_root = work_root.with_join_str("python");
            python_root.fs_create_dir_all()?;
            crate::linker::helpers::fs_extract_archive(&python_root, managed_python)?;
            Some(find_python_executable_path(&python_root, target).ok_or_else(|| {
                invalid_sdist(filename, "managed Python archive contains no supported interpreter")
            })?)
        } else {
            None
        };

        let runner_for_task = runner.clone();
        let extraction_for_task = extraction_root.clone();
        let output_for_task = output_root.clone();
        let work_for_task = work_root.clone();
        let python_for_task = preferred_python.clone();
        let target_for_task = target.cloned();
        let output = tokio::task::spawn_blocking(move || {
            run_pep517_runner(
                &runner_for_task,
                &extraction_for_task,
                &output_for_task,
                &work_for_task,
                "build_wheel",
                python_for_task.as_ref(),
                target_for_task.as_ref(),
            )
        }).await??;

        let wheel_name = wheel_name_from_output(&output, filename)?;
        let wheel = output_root.with_join_str(&wheel_name).fs_read()?;
        validate_wheel(&wheel, &wheel_name, expected_ident, expected_version, target, filename)?;
        Ok(wheel)
    }.await;

    let _ = work_root.fs_rm();
    result
}

/// Builds an editable wheel for a local Python project.
///
/// Backends that don't implement PEP 660's `build_editable` hook fall back to
/// `build_wheel`, which still gives the workspace a usable installed package.
pub async fn prepare_project(
    project_root: &Path,
    python: Option<&Path>,
    target: Option<&PythonTargetEnv>,
) -> Result<Vec<u8>, Error> {
    let work_root = Path::temp_dir_pattern("zpm-python-project-<>")?;
    let output_root = work_root.with_join_str("wheel");
    let runner = work_root.with_join_str("pep517_runner.py");

    let result = async {
        output_root.fs_create_dir_all()?;
        runner.fs_write_text(PEP517_RUNNER)?;

        let runner_for_task = runner.clone();
        let project_for_task = project_root.clone();
        let output_for_task = output_root.clone();
        let work_for_task = work_root.clone();
        let python_for_task = python.cloned();
        let target_for_task = target.cloned();
        let output = tokio::task::spawn_blocking(move || {
            run_pep517_runner(
                &runner_for_task,
                &project_for_task,
                &output_for_task,
                &work_for_task,
                "build_editable",
                python_for_task.as_ref(),
                target_for_task.as_ref(),
            )
        }).await??;

        let wheel_name = wheel_name_from_output(&output, &project_root.to_file_string())?;
        let wheel = output_root.with_join_str(&wheel_name).fs_read()?;

        // Local projects don't need to match the JavaScript workspace name,
        // but the backend must still have emitted a valid, target-compatible
        // wheel with Python distribution metadata.
        let entries = zpm_formats::zip::entries_from_zip(&wheel)
            .map_err(|error| invalid_sdist(&project_root.to_file_string(), format!("backend produced an invalid wheel: {error}")))?;
        if !entries.iter().any(|entry| entry.name.as_str().ends_with(".dist-info/METADATA")) {
            return Err(invalid_sdist(
                &project_root.to_file_string(),
                "backend produced a wheel without .dist-info/METADATA",
            ));
        }

        if let Some(target) = target {
            let distribution = crate::pypi::PypiDistribution {
                filename: wheel_name.clone(),
                packagetype: "bdist_wheel".to_string(),
                url: String::new(),
                upload_time: None,
                upload_time_iso_8601: None,
                requires_python: None,
            };
            if crate::pypi::select_best_wheel(&[distribution], Some(target)).is_none() {
                return Err(invalid_sdist(
                    &project_root.to_file_string(),
                    format!("backend produced wheel `{wheel_name}`, which is incompatible with the selected Python target"),
                ));
            }
        }

        Ok(wheel)
    }.await;

    let _ = work_root.fs_rm();
    result
}

/// Builds a regular wheel from an already-materialized Python source tree.
///
/// Unlike [`prepare_project`], this uses PEP 517's `build_wheel` hook: Git
/// dependencies are immutable snapshots and must not retain editable links to
/// the temporary checkout used during installation.
pub async fn prepare_source_tree(
    project_root: &Path,
    target: Option<&PythonTargetEnv>,
    managed_python: Option<&PackageData>,
) -> Result<Vec<u8>, Error> {
    let work_root = Path::temp_dir_pattern("zpm-python-source-<>")?;
    let output_root = work_root.with_join_str("wheel");
    let runner = work_root.with_join_str("pep517_runner.py");

    let result = async {
        output_root.fs_create_dir_all()?;
        runner.fs_write_text(PEP517_RUNNER)?;

        let preferred_python = if let Some(managed_python) = managed_python {
            let python_root = work_root.with_join_str("python");
            python_root.fs_create_dir_all()?;
            crate::linker::helpers::fs_extract_archive(&python_root, managed_python)?;
            Some(find_python_executable_path(&python_root, target).ok_or_else(|| {
                Error::PythonPreparation("managed Python archive contains no supported interpreter".to_string())
            })?)
        } else {
            None
        };

        let runner_for_task = runner.clone();
        let project_for_task = project_root.clone();
        let output_for_task = output_root.clone();
        let work_for_task = work_root.clone();
        let python_for_task = preferred_python.clone();
        let target_for_task = target.cloned();
        let output = tokio::task::spawn_blocking(move || {
            run_pep517_runner(
                &runner_for_task,
                &project_for_task,
                &output_for_task,
                &work_for_task,
                "build_wheel",
                python_for_task.as_ref(),
                target_for_task.as_ref(),
            )
        }).await??;

        let wheel_name = wheel_name_from_output(&output, &project_root.to_file_string())?;
        let wheel = output_root.with_join_str(&wheel_name).fs_read()?;
        let entries = zpm_formats::zip::entries_from_zip(&wheel)
            .map_err(|error| Error::PythonPreparation(format!("source project produced an invalid wheel: {error}")))?;
        if !entries.iter().any(|entry| entry.name.as_str().ends_with(".dist-info/METADATA")) {
            return Err(Error::PythonPreparation("source project produced a wheel without .dist-info/METADATA".to_string()));
        }

        if let Some(target) = target {
            let distribution = crate::pypi::PypiDistribution {
                filename: wheel_name.clone(),
                packagetype: "bdist_wheel".to_string(),
                url: String::new(),
                upload_time: None,
                upload_time_iso_8601: None,
                requires_python: None,
            };
            if crate::pypi::select_best_wheel(&[distribution], Some(target)).is_none() {
                return Err(Error::PythonPreparation(format!(
                    "source project produced wheel `{wheel_name}`, which is incompatible with the selected Python target",
                )));
            }
        }

        Ok(wheel)
    }.await;

    let _ = work_root.fs_rm();
    result
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use zpm_formats::Entry;

    use super::*;

    #[test]
    fn test_validate_archive_entries_rejects_parent_traversal() {
        let entries = vec![Entry::new_file(
            Path::from_file_string("../outside.py").unwrap(),
            Cow::Borrowed(b"bad"),
        )];

        let error = validate_archive_entries("bad.tar.gz", &entries).unwrap_err();
        assert!(error.to_string().contains("escapes the source directory"));
    }
}
