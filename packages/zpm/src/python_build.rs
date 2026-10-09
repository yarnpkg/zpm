//! Builds wheels out of source distributions through PEP 517.
//!
//! Source distributions (and Git sources) are fetched like any other
//! package; only when they're about to be installed in a venv are they
//! turned into a wheel. The build runs in an isolated, temporary venv:
//!
//! 1. create a venv with the island's interpreter (`python -m venv`, which
//!    bootstraps pip through `ensurepip`, so no network access to fetch pip);
//! 2. install the `build-system.requires` with pip, pointed at the same
//!    index Yarn uses (credentials passed through the URL, never through
//!    the backend's environment);
//! 3. call the backend's `build_wheel` hook through a small driver script.
//!
//! The produced wheel is cached in `<globalFolder>/pypi-built`, keyed by
//! the sdist checksum and the interpreter version, so each sdist is built
//! once per machine.

use std::process::Command;

use zpm_primitives::Locator;
use zpm_utils::{Hash64, Path, ToFileString, ToHumanString};

use crate::{
    error::Error,
    project::Project,
    pypi::{self, get_registry},
    python_interpreter::Interpreter,
};

const BUILD_DRIVER: &str = r#"
import importlib, json, os, sys

# The build-system table comes from Yarn, as JSON: tomllib only exists
# on Python 3.11+
source, out, mode, result_path, build_system = sys.argv[1:6]
build_system = json.loads(build_system)
sys.argv = sys.argv[:1]
os.chdir(source)

backend_name = build_system.get("build-backend", "setuptools.build_meta:__legacy__")
for path in reversed(build_system.get("backend-path", [])):
    sys.path.insert(0, os.path.abspath(path))

module_name, _, attr = backend_name.partition(":")
backend = importlib.import_module(module_name)
for part in filter(None, attr.split(".")):
    backend = getattr(backend, part)

if mode == "requires":
    hook = getattr(backend, "get_requires_for_build_wheel", None)
    result = "\n".join(hook({}) if hook else [])
else:
    result = backend.build_wheel(out, {})

with open(result_path, "w") as f:
    f.write(result)
"#;

/// The `[build-system]` table of a source tree, as JSON (empty when
/// missing), for the build driver.
fn read_build_system(source: &Path) -> String {
    let Ok(text) = source.with_join_str("pyproject.toml").fs_read_text() else {
        return "{}".to_string();
    };

    let Ok(document) = text.parse::<toml_edit::DocumentMut>() else {
        return "{}".to_string();
    };

    let build_system
        = document.get("build-system").and_then(|item| item.as_table_like());

    let Some(build_system) = build_system else {
        return "{}".to_string();
    };

    let mut json
        = serde_json::Map::new();

    if let Some(backend) = build_system.get("build-backend").and_then(|value| value.as_str()) {
        json.insert("build-backend".to_string(), serde_json::Value::String(backend.to_string()));
    }

    if let Some(paths) = build_system.get("backend-path").and_then(|value| value.as_array()) {
        let paths
            = paths.iter()
                .filter_map(|value| value.as_str().map(|value| serde_json::Value::String(value.to_string())))
                .collect();

        json.insert("backend-path".to_string(), serde_json::Value::Array(paths));
    }

    serde_json::Value::Object(json).to_string()
}

fn read_build_requires(source: &Path) -> Vec<String> {
    let Ok(text) = source.with_join_str("pyproject.toml").fs_read_text() else {
        return vec!["setuptools>=40.8.0".to_string(), "wheel".to_string()];
    };

    let Ok(document) = text.parse::<toml_edit::DocumentMut>() else {
        return vec!["setuptools>=40.8.0".to_string(), "wheel".to_string()];
    };

    let Some(build_system) = document.get("build-system") else {
        return vec!["setuptools>=40.8.0".to_string(), "wheel".to_string()];
    };

    build_system.get("requires")
        .and_then(|requires| requires.as_array())
        .map(|requires| requires.iter().filter_map(|value| value.as_str().map(|value| value.to_string())).collect())
        .unwrap_or_default()
}

fn run(command: &mut Command, what: &str) -> Result<String, Error> {
    let output
        = command.output()
            .map_err(|err| Error::InvalidResolution(format!("Failed to {}: {}", what, err)))?;

    if !output.status.success() {
        return Err(Error::InvalidResolution(format!(
            "Failed to {}:\n{}{}",
            what,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        )));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// The index URL handed to pip, with the credentials Yarn would use.
fn pip_index_url(project: &Project, locator: &Locator) -> String {
    let registry
        = get_registry(&project.config, &locator.ident);

    // Same credentials as the ones Yarn uses for this index and package
    // (including `sourceRules` and `packageRules`)
    let token = match pypi::get_credentials(&project.config, &registry, Some(&locator.ident)) {
        Some(pypi::PypiCredentials::Ident(auth_ident)) => Some(auth_ident),
        Some(pypi::PypiCredentials::Token(token)) => Some(format!("__token__:{}", token)),
        None => None,
    };

    match (token, url::Url::parse(&registry)) {
        (Some(token), Ok(mut url)) => {
            let (user, password) = token.split_once(':').unwrap_or(("__token__", token.as_str()));
            let _ = url.set_username(user);
            let _ = url.set_password(Some(password));
            url.to_string()
        },

        _ => registry,
    }
}

fn extract_sdist(archive: &Path, destination: &Path) -> Result<Path, Error> {
    destination.fs_create_dir_all()?;

    let mut command
        = Command::new("tar");

    command.arg("-xzf").arg(archive.to_path_buf()).arg("-C").arg(destination.to_path_buf());

    if archive.to_file_string().ends_with(".src.zip") {
        command = Command::new("unzip");
        command.arg("-q").arg(archive.to_path_buf()).arg("-d").arg(destination.to_path_buf());
    }

    run(&mut command, "extract the source distribution")?;

    // sdists contain a single top-level `<name>-<version>/` folder
    let entries
        = destination.fs_read_dir()?
            .flatten()
            .filter(|entry| entry.path().is_dir())
            .collect::<Vec<_>>();

    match entries.as_slice() {
        [single] => Ok(Path::try_from(single.path())?),
        _ => Ok(destination.clone()),
    }
}

/// Builds (or retrieves from the cache) the wheel of a source tree.
pub fn build_wheel_from_source(project: &Project, interpreter: &Interpreter, locator: &Locator, source: &Path, cache_key: &Hash64) -> Result<Path, Error> {
    let built_root
        = project.config.settings.global_folder.value
            .with_join_str("pypi-built");

    let cached_wheel
        = built_root.with_join_str(format!("{}-{}-py{}.whl", locator.ident.slug(), cache_key.short(), interpreter.version.short()));

    if cached_wheel.fs_exists() {
        return Ok(cached_wheel);
    }

    let work
        = Path::temp_dir()?;

    let result = (|| -> Result<Path, Error> {
        let build_env
            = work.with_join_str("env");

        run(Command::new(interpreter.executable.to_path_buf()).arg("-m").arg("venv").arg(build_env.to_path_buf()), "create the build environment")?;

        let python
            = build_env.with_join_str("bin/python");

        let index_url
            = pip_index_url(project, locator);

        let requires
            = read_build_requires(source);

        let pip_install = |requirements: &[String]| -> Result<(), Error> {
            if requirements.is_empty() {
                return Ok(());
            }

            run(Command::new(python.to_path_buf())
                .args(["-m", "pip", "install", "--quiet", "--disable-pip-version-check", "--no-input", "--index-url"])
                .arg(&index_url)
                .args(requirements), "install the build dependencies")?;

            Ok(())
        };

        pip_install(&requires)?;

        let driver
            = work.with_join_str("driver.py");

        driver.fs_write(BUILD_DRIVER)?;

        let out
            = work.with_join_str("out");

        out.fs_create_dir_all()?;

        let result_path
            = work.with_join_str("result.txt");

        let build_system
            = read_build_system(source);

        run(Command::new(python.to_path_buf()).arg(driver.to_path_buf()).arg(source.to_path_buf()).arg(out.to_path_buf()).arg("requires").arg(result_path.to_path_buf()).arg(&build_system), "query the build backend")?;

        let dynamic_requires
            = result_path.fs_read_text()?;

        let dynamic_requires
            = dynamic_requires.lines()
                .map(|line| line.trim().to_string())
                .filter(|line| !line.is_empty() && !requires.contains(line))
                .collect::<Vec<_>>();

        pip_install(&dynamic_requires)?;

        run(Command::new(python.to_path_buf()).arg(driver.to_path_buf()).arg(source.to_path_buf()).arg(out.to_path_buf()).arg("wheel").arg(result_path.to_path_buf()).arg(&build_system), &format!("build {}", locator.to_print_string()))?;

        let wheel_name
            = result_path.fs_read_text()?.trim().to_string();

        let wheel
            = out.with_join_str(&wheel_name);

        built_root.fs_create_dir_all()?;

        let tmp
            = built_root.with_join_str(format!(".tmp-{}-{}", std::process::id(), cache_key.short()));

        wheel.fs_copy_file(&tmp)?;
        tmp.fs_rename(&cached_wheel)?;

        Ok(cached_wheel.clone())
    })();

    let _ = work.fs_rm();

    result
}

/// Builds the wheel of a source distribution archive.
pub fn build_wheel_from_sdist(project: &Project, interpreter: &Interpreter, locator: &Locator, archive: &Path, checksum: &Hash64) -> Result<Path, Error> {
    let built_root
        = project.config.settings.global_folder.value
            .with_join_str("pypi-built");

    let cached_wheel
        = built_root.with_join_str(format!("{}-{}-py{}.whl", locator.ident.slug(), checksum.short(), interpreter.version.short()));

    if cached_wheel.fs_exists() {
        return Ok(cached_wheel);
    }

    let work
        = Path::temp_dir()?;

    let result = extract_sdist(archive, &work.with_join_str("src"))
        .and_then(|source| build_wheel_from_source(project, interpreter, locator, &source, checksum));

    let _ = work.fs_rm();

    result
}
