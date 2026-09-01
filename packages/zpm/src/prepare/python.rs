use std::process::{Command, Output};

use serde::Deserialize;
use zpm_primitives::{Ident, PypiVersion, PythonTargetEnv, canonicalize_pypi_name};
use zpm_utils::{FromFileString, Path, ToFileString};

use crate::error::Error;
use crate::fetchers::PackageData;

const LEGACY_BUILD_BACKEND: &str = "setuptools.build_meta:__legacy__";

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
struct BuildSystem {
    requires: Vec<String>,

    #[serde(default = "default_build_backend")]
    build_backend: String,

    #[serde(default)]
    backend_path: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct PyprojectDocument {
    build_system: Option<BuildSystem>,
}

fn default_build_backend() -> String {
    LEGACY_BUILD_BACKEND.to_string()
}

fn legacy_build_system() -> BuildSystem {
    BuildSystem {
        requires: vec!["setuptools>=40.8.0".to_string()],
        build_backend: default_build_backend(),
        backend_path: Vec::new(),
    }
}

fn has_python_project_marker(path: &Path) -> bool {
    ["pyproject.toml", "setup.py", "setup.cfg"]
        .into_iter()
        .any(|marker| path.with_join_str(marker).fs_is_file())
}

fn find_project_root(extraction_root: &Path) -> Result<Path, Error> {
    if has_python_project_marker(extraction_root) {
        return Ok(extraction_root.clone());
    }

    let mut candidates = Vec::new();
    for entry in extraction_root.fs_read_dir()? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }

        let path = Path::try_from(entry.path())?;
        if has_python_project_marker(&path) {
            candidates.push(path);
        }
    }

    if candidates.len() != 1 {
        return Err(Error::PythonPreparation(
            "source archive must contain exactly one Python project root".to_string(),
        ));
    }

    Ok(candidates.remove(0))
}

fn parse_build_system(contents: &str, pyproject_path: &Path) -> Result<BuildSystem, Error> {
    let document: PyprojectDocument = toml::from_str(contents).map_err(|error| {
        Error::PythonPreparation(format!(
            "Cannot parse {}: {error}",
            pyproject_path.to_file_string(),
        ))
    })?;
    let build_system = document.build_system.unwrap_or_else(legacy_build_system);

    if build_system.build_backend.is_empty() {
        return Err(Error::PythonPreparation(format!(
            "{} build-system.build-backend must be a non-empty string",
            pyproject_path.to_file_string(),
        )));
    }

    Ok(build_system)
}

fn read_build_system(project_root: &Path) -> Result<BuildSystem, Error> {
    let pyproject_path = project_root.with_join_str("pyproject.toml");
    if !pyproject_path.fs_is_file() {
        return Ok(legacy_build_system());
    }

    parse_build_system(&pyproject_path.fs_read_text()?, &pyproject_path)
}

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

fn select_python(
    preferred_python: Option<&Path>,
    target: Option<&PythonTargetEnv>,
) -> Result<String, Error> {
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

        match Command::new(&python).arg("--version").output() {
            Ok(output) if output.status.success() => return Ok(python),
            Ok(_) => {},
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

fn check_command_output(output: Output, subject: &str, fallback: &str) -> Result<(), Error> {
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let detail = if !stderr.trim().is_empty() {
            stderr.trim()
        } else if !stdout.trim().is_empty() {
            stdout.trim()
        } else {
            fallback
        };
        return Err(invalid_sdist(subject, detail));
    }

    Ok(())
}

fn configure_build_command(
    command: &mut Command,
    build_python: &Path,
    build_environment: &Path,
    build_index_url: &str,
) -> Result<(), Error> {
    let scripts_path = build_python.dirname().unwrap_or_else(|| build_environment.clone());
    let mut paths = vec![scripts_path.to_path_buf()];
    if let Some(path) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&path));
    }
    let path = std::env::join_paths(paths)
        .map_err(|error| Error::PythonPreparation(format!("Cannot construct PEP 517 PATH: {error}")))?;

    command
        .current_dir(build_environment.to_path_buf())
        .env("PATH", path)
        .env("VIRTUAL_ENV", build_environment.to_path_buf())
        .env("PIP_INDEX_URL", build_index_url)
        .env_remove("PIP_EXTRA_INDEX_URL")
        .env_remove("PIP_NO_INDEX");

    Ok(())
}

fn create_build_environment(
    python: &str,
    work_root: &Path,
    build_index_url: &str,
    subject: &str,
) -> Result<(Path, Path), Error> {
    let build_environment = work_root.with_join_str("build-environment");
    let mut command = Command::new(python);
    command
        .args(["-m", "venv", "--clear"])
        .arg(build_environment.to_path_buf())
        .env("PIP_INDEX_URL", build_index_url)
        .env_remove("PIP_EXTRA_INDEX_URL")
        .env_remove("PIP_NO_INDEX");
    check_command_output(
        command.output()?,
        subject,
        "unable to create the PEP 517 build environment",
    )?;

    let build_python = find_python_executable_path(&build_environment, None)
        .ok_or_else(|| invalid_sdist(subject, "PEP 517 build environment contains no Python interpreter"))?;
    Ok((build_environment, build_python))
}

fn install_build_requirements(
    build_python: &Path,
    build_environment: &Path,
    requirements: &[String],
    build_index_url: &str,
    subject: &str,
) -> Result<(), Error> {
    if requirements.is_empty() {
        return Ok(());
    }

    let mut command = Command::new(build_python.to_path_buf());
    command.args([
        "-m",
        "pip",
        "install",
        "--disable-pip-version-check",
        "--no-input",
    ]);
    command.args(requirements);
    configure_build_command(
        &mut command,
        build_python,
        build_environment,
        build_index_url,
    )?;
    check_command_output(
        command.output()?,
        subject,
        "unable to install PEP 517 build requirements",
    )
}

fn resolve_backend_paths(project_root: &Path, build_system: &BuildSystem) -> Result<Vec<String>, Error> {
    let canonical_root = project_root.fs_canonicalize()?;
    let mut backend_paths = Vec::new();

    for raw_path in &build_system.backend_path {
        let relative = Path::from_file_string(raw_path).map_err(|_| {
            Error::PythonPreparation(format!(
                "build-system.backend-path entry `{raw_path}` is not a valid path",
            ))
        })?;
        if !relative.is_forward() || relative.segments().any(|segment| segment == "..") {
            return Err(Error::PythonPreparation(format!(
                "build-system.backend-path entry `{raw_path}` must stay within the source tree",
            )));
        }

        let candidate = project_root.with_join(&relative);
        if !candidate.fs_is_dir() {
            return Err(Error::PythonPreparation(format!(
                "build-system.backend-path entry `{raw_path}` is not a directory",
            )));
        }
        let candidate = candidate.fs_canonicalize()?;
        if candidate.strip_prefix(&canonical_root).is_none() {
            return Err(Error::PythonPreparation(format!(
                "build-system.backend-path entry `{raw_path}` must stay within the source tree",
            )));
        }
        backend_paths.push(candidate.to_file_string());
    }

    Ok(backend_paths)
}

fn backend_import_script(project_root: &Path, build_system: &BuildSystem) -> Result<String, Error> {
    let backend = serde_json::to_string(&build_system.build_backend)
        .map_err(|error| Error::SerializationError(error.to_string()))?;
    let backend_paths = serde_json::to_string(&resolve_backend_paths(project_root, build_system)?)
        .map_err(|error| Error::SerializationError(error.to_string()))?;

    Ok(format!(r#"
import importlib
import sys

if sys.path and sys.path[0] == "":
    sys.path.pop(0)
sys.path[:0] = {backend_paths}

module_name, separator, object_path = {backend}.partition(":")
backend = importlib.import_module(module_name)
if separator:
    for component in object_path.split("."):
        backend = getattr(backend, component)
"#))
}

fn hook_selection_script(requested_hook: &str) -> Result<String, Error> {
    let requested_hook = serde_json::to_string(requested_hook)
        .map_err(|error| Error::SerializationError(error.to_string()))?;

    Ok(format!(r#"
requested_hook = {requested_hook}
selected_hook = requested_hook
hook = getattr(backend, selected_hook, None)
if hook is None and requested_hook == "build_editable":
    selected_hook = "build_wheel"
    hook = getattr(backend, selected_hook, None)
if hook is None:
    raise RuntimeError(f"PEP 517 backend has no {{requested_hook}} hook")
"#))
}

fn run_backend_script(
    build_python: &Path,
    build_environment: &Path,
    project_root: &Path,
    script: &str,
    build_index_url: &str,
    subject: &str,
) -> Result<(), Error> {
    let mut command = Command::new(build_python.to_path_buf());
    command.args(["-c", script]);
    configure_build_command(
        &mut command,
        build_python,
        build_environment,
        build_index_url,
    )?;
    command.current_dir(project_root.to_path_buf());
    check_command_output(command.output()?, subject, "PEP 517 backend failed")
}

fn get_dynamic_build_requirements(
    build_python: &Path,
    build_environment: &Path,
    project_root: &Path,
    work_root: &Path,
    build_system: &BuildSystem,
    requested_hook: &str,
    build_index_url: &str,
    subject: &str,
) -> Result<Vec<String>, Error> {
    let result_path = work_root.with_join_str("dynamic-build-requirements.json");
    let result_path_literal = serde_json::to_string(&result_path.to_file_string())
        .map_err(|error| Error::SerializationError(error.to_string()))?;
    let script = format!(r#"
{}
{}
import json

requirements_hook = getattr(backend, f"get_requires_for_{{selected_hook}}", None)
requirements = requirements_hook({{}}) if requirements_hook is not None else []
with open({result_path_literal}, "w", encoding="utf-8") as stream:
    json.dump(requirements, stream)
"#,
        backend_import_script(project_root, build_system)?,
        hook_selection_script(requested_hook)?,
    );

    run_backend_script(
        build_python,
        build_environment,
        project_root,
        &script,
        build_index_url,
        subject,
    )?;

    serde_json::from_str(&result_path.fs_read_text()?).map_err(|_| {
        invalid_sdist(
            subject,
            format!("PEP 517 get_requires_for_{requested_hook} must return an array of strings"),
        )
    })
}

fn run_build_hook(
    build_python: &Path,
    build_environment: &Path,
    project_root: &Path,
    output_root: &Path,
    work_root: &Path,
    build_system: &BuildSystem,
    requested_hook: &str,
    build_index_url: &str,
    subject: &str,
) -> Result<String, Error> {
    let output_root_literal = serde_json::to_string(&output_root.to_file_string())
        .map_err(|error| Error::SerializationError(error.to_string()))?;
    let result_path = work_root.with_join_str("wheel-name.json");
    let result_path_literal = serde_json::to_string(&result_path.to_file_string())
        .map_err(|error| Error::SerializationError(error.to_string()))?;
    let script = format!(r#"
{}
{}
import json

wheel_name = hook({output_root_literal}, {{}}, None)
with open({result_path_literal}, "w", encoding="utf-8") as stream:
    json.dump(wheel_name, stream)
"#,
        backend_import_script(project_root, build_system)?,
        hook_selection_script(requested_hook)?,
    );

    run_backend_script(
        build_python,
        build_environment,
        project_root,
        &script,
        build_index_url,
        subject,
    )?;

    let wheel_name: String = serde_json::from_str(&result_path.fs_read_text()?)
        .map_err(|_| invalid_sdist(subject, "PEP 517 build hook must return a wheel filename"))?;
    if wheel_name.is_empty()
        || !wheel_name.ends_with(".whl")
        || wheel_name.contains('/')
        || wheel_name.contains('\\')
    {
        return Err(invalid_sdist(subject, "PEP 517 backend returned an invalid wheel filename"));
    }
    if !output_root.with_join_str(&wheel_name).fs_is_file() {
        return Err(invalid_sdist(
            subject,
            format!("PEP 517 backend did not create {wheel_name}"),
        ));
    }

    Ok(wheel_name)
}

fn run_pep517_build(
    project_root: &Path,
    output_root: &Path,
    work_root: &Path,
    build_system: &BuildSystem,
    requested_hook: &str,
    preferred_python: Option<&Path>,
    target: Option<&PythonTargetEnv>,
    build_index_url: &str,
    subject: &str,
) -> Result<String, Error> {
    let python = select_python(preferred_python, target)?;
    let (build_environment, build_python) = create_build_environment(
        &python,
        work_root,
        build_index_url,
        subject,
    )?;
    install_build_requirements(
        &build_python,
        &build_environment,
        &build_system.requires,
        build_index_url,
        subject,
    )?;
    let dynamic_requirements = get_dynamic_build_requirements(
        &build_python,
        &build_environment,
        project_root,
        work_root,
        build_system,
        requested_hook,
        build_index_url,
        subject,
    )?;
    install_build_requirements(
        &build_python,
        &build_environment,
        &dynamic_requirements,
        build_index_url,
        subject,
    )?;
    run_build_hook(
        &build_python,
        &build_environment,
        project_root,
        output_root,
        work_root,
        build_system,
        requested_hook,
        build_index_url,
        subject,
    )
}

/// Parses a freshly-built wheel, checks that it carries Python distribution
/// metadata and that its tags are installable on the selected target, and
/// returns the unfolded METADATA headers. Errors are plain messages so each
/// caller can wrap them with its own subject.
fn validate_built_wheel(
    wheel: &[u8],
    wheel_name: &str,
    target: Option<&PythonTargetEnv>,
) -> Result<Vec<String>, String> {
    let entries = zpm_formats::zip::entries_from_zip(wheel)
        .map_err(|error| format!("backend produced an invalid wheel: {error}"))?;
    let metadata_entry = entries.iter()
        .find(|entry| entry.name.as_str().ends_with(".dist-info/METADATA"))
        .ok_or_else(|| "backend produced a wheel without .dist-info/METADATA".to_string())?;
    let metadata = std::str::from_utf8(&metadata_entry.data)
        .map_err(|_| "wheel METADATA is not UTF-8".to_string())?;

    if let Some(target) = target {
        let distribution = crate::pypi::PypiDistribution {
            filename: wheel_name.to_string(),
            packagetype: "bdist_wheel".to_string(),
            url: String::new(),
            upload_time: None,
            upload_time_iso_8601: None,
            requires_python: None,
        };
        if crate::pypi::select_best_wheel(&[distribution], Some(target)).is_none() {
            return Err(format!(
                "backend produced wheel `{wheel_name}`, which is incompatible with the selected Python target",
            ));
        }
    }

    Ok(crate::pypi::unfold_metadata_headers(metadata))
}

fn validate_wheel(
    wheel: &[u8],
    wheel_filename: &str,
    expected_ident: &Ident,
    expected_version: &PypiVersion,
    target: Option<&PythonTargetEnv>,
    sdist_filename: &str,
) -> Result<(), Error> {
    let headers = validate_built_wheel(wheel, wheel_filename, target)
        .map_err(|message| invalid_sdist(sdist_filename, message))?;

    let name = crate::pypi::metadata_header_field(&headers, "Name")
        .ok_or_else(|| invalid_sdist(sdist_filename, "wheel METADATA has no Name field"))?;
    if canonicalize_pypi_name(name) != expected_ident.as_str() {
        return Err(invalid_sdist(sdist_filename, format!(
            "backend produced package `{name}`, expected `{}`",
            expected_ident.to_file_string(),
        )));
    }

    let version = crate::pypi::metadata_header_field(&headers, "Version")
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
    build_index_url: &str,
) -> Result<Vec<u8>, Error> {
    let work_root = Path::temp_dir_pattern("zpm-python-sdist-<>")?;
    let extraction_root = work_root.with_join_str("source");
    let output_root = work_root.with_join_str("wheel");

    let result = async {
        extraction_root.fs_create_dir_all()?;
        output_root.fs_create_dir_all()?;
        unpack_sdist(source, filename, &extraction_root)?;
        let project_root = find_project_root(&extraction_root)?;
        let build_system = read_build_system(&project_root)?;

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

        let project_for_task = project_root.clone();
        let output_for_task = output_root.clone();
        let work_for_task = work_root.clone();
        let build_system_for_task = build_system;
        let python_for_task = preferred_python.clone();
        let target_for_task = target.cloned();
        let build_index_for_task = build_index_url.to_string();
        let subject_for_task = filename.to_string();
        let wheel_name = tokio::task::spawn_blocking(move || {
            run_pep517_build(
                &project_for_task,
                &output_for_task,
                &work_for_task,
                &build_system_for_task,
                "build_wheel",
                python_for_task.as_ref(),
                target_for_task.as_ref(),
                &build_index_for_task,
                &subject_for_task,
            )
        }).await??;

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
    build_index_url: &str,
) -> Result<Vec<u8>, Error> {
    let work_root = Path::temp_dir_pattern("zpm-python-project-<>")?;
    let output_root = work_root.with_join_str("wheel");

    let result = async {
        output_root.fs_create_dir_all()?;
        let project_root = find_project_root(project_root)?;
        let build_system = read_build_system(&project_root)?;
        let subject = project_root.to_file_string();

        let project_for_task = project_root.clone();
        let output_for_task = output_root.clone();
        let work_for_task = work_root.clone();
        let build_system_for_task = build_system;
        let python_for_task = python.cloned();
        let target_for_task = target.cloned();
        let build_index_for_task = build_index_url.to_string();
        let subject_for_task = subject.clone();
        let wheel_name = tokio::task::spawn_blocking(move || {
            run_pep517_build(
                &project_for_task,
                &output_for_task,
                &work_for_task,
                &build_system_for_task,
                "build_editable",
                python_for_task.as_ref(),
                target_for_task.as_ref(),
                &build_index_for_task,
                &subject_for_task,
            )
        }).await??;

        let wheel = output_root.with_join_str(&wheel_name).fs_read()?;

        // Local projects don't need to match the JavaScript workspace name,
        // but the backend must still have emitted a valid, target-compatible
        // wheel with Python distribution metadata.
        validate_built_wheel(&wheel, &wheel_name, target)
            .map_err(|message| invalid_sdist(&subject, message))?;

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
    build_index_url: &str,
) -> Result<Vec<u8>, Error> {
    let work_root = Path::temp_dir_pattern("zpm-python-source-<>")?;
    let output_root = work_root.with_join_str("wheel");

    let result = async {
        output_root.fs_create_dir_all()?;
        let project_root = find_project_root(project_root)?;
        let build_system = read_build_system(&project_root)?;
        let subject = project_root.to_file_string();

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

        let project_for_task = project_root.clone();
        let output_for_task = output_root.clone();
        let work_for_task = work_root.clone();
        let build_system_for_task = build_system;
        let python_for_task = preferred_python.clone();
        let target_for_task = target.cloned();
        let build_index_for_task = build_index_url.to_string();
        let subject_for_task = subject.clone();
        let wheel_name = tokio::task::spawn_blocking(move || {
            run_pep517_build(
                &project_for_task,
                &output_for_task,
                &work_for_task,
                &build_system_for_task,
                "build_wheel",
                python_for_task.as_ref(),
                target_for_task.as_ref(),
                &build_index_for_task,
                &subject_for_task,
            )
        }).await??;

        let wheel = output_root.with_join_str(&wheel_name).fs_read()?;
        validate_built_wheel(&wheel, &wheel_name, target)
            .map_err(|message| Error::PythonPreparation(format!("{subject}: {message}")))?;

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

    #[test]
    fn test_parse_build_system_reads_pep517_configuration() {
        let path = Path::from_file_string("pyproject.toml").unwrap();
        let build_system = parse_build_system(r#"
            [build-system]
            requires = ["hatchling>=1.0", "packaging"]
            build-backend = "hatchling.build"
            backend-path = ["backend"]
        "#, &path).unwrap();

        assert_eq!(build_system, BuildSystem {
            requires: vec!["hatchling>=1.0".to_string(), "packaging".to_string()],
            build_backend: "hatchling.build".to_string(),
            backend_path: vec!["backend".to_string()],
        });
    }

    #[test]
    fn test_parse_build_system_uses_legacy_defaults_without_table() {
        let path = Path::from_file_string("pyproject.toml").unwrap();
        let build_system = parse_build_system("[project]\nname = \"demo\"", &path).unwrap();

        assert_eq!(build_system, legacy_build_system());
    }

    #[test]
    fn test_parse_build_system_rejects_invalid_requirements() {
        let path = Path::from_file_string("pyproject.toml").unwrap();
        let error = parse_build_system(r#"
            [build-system]
            requires = "setuptools"
        "#, &path).unwrap_err();

        assert!(error.to_string().contains("requires"));
        assert!(error.to_string().contains("sequence"));
    }

    #[test]
    fn test_resolve_backend_paths_rejects_paths_outside_source_tree() {
        let project_root = Path::temp_dir_pattern("zpm-python-backend-path-<>").unwrap();
        let build_system = BuildSystem {
            requires: Vec::new(),
            build_backend: "backend".to_string(),
            backend_path: vec!["../backend".to_string()],
        };

        let error = resolve_backend_paths(&project_root, &build_system).unwrap_err();
        let _ = project_root.fs_rm();

        assert!(error.to_string().contains("must stay within the source tree"));
    }
}
