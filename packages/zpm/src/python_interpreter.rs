//! Locates (or provisions) the Python interpreter used by venv islands.
//!
//! Lookup order, depending on `pythonPreference`:
//!
//! - `managed` (default): a python-build-standalone build already present
//!   in `<globalFolder>/python`, then a matching interpreter on the PATH,
//!   then a download (when `pythonDownloads` is enabled).
//! - `system`: a matching interpreter on the PATH first.
//!
//! Managed builds are shared by every project on the machine; they're
//! never copied into the venvs, which only point to them (`pyvenv.cfg`).

use std::{process::Command, sync::{Arc, LazyLock}};

use dashmap::DashMap;
use serde::Deserialize;
use tokio::sync::OnceCell;
use zpm_config::{Configuration, PythonPreference};
use zpm_utils::{FromFileString, Path, ToFileString};

use crate::{
    error::Error,
    http::HttpClient,
    python_env::{Platform, PythonVersion},
};

#[derive(Clone, Debug)]
pub struct Interpreter {
    pub executable: Path,
    pub home: Path,
    pub version: PythonVersion,
}

fn version_matches(requested: &PythonVersion, found: &PythonVersion) -> bool {
    requested.major == found.major
        && requested.minor == found.minor
        && requested.patch.map_or(true, |patch| Some(patch) == found.patch)
}

fn probe(executable: &Path) -> Option<Interpreter> {
    let output
        = Command::new(executable.to_path_buf())
            .args(["-I", "-c", "import sys, platform; print(platform.python_version()); print(sys.base_prefix); print(sys.executable)"])
            .output()
            .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout
        = String::from_utf8_lossy(&output.stdout).to_string();

    let mut lines
        = stdout.lines();

    let version
        = PythonVersion::parse(lines.next()?)?;
    let _prefix
        = lines.next()?;
    let resolved
        = Path::from_file_string(lines.next()?).ok()?;

    let home
        = resolved.dirname()?;

    Some(Interpreter {
        executable: resolved,
        home,
        version,
    })
}

fn find_on_path(requested: &PythonVersion) -> Option<Interpreter> {
    let path
        = std::env::var("PATH").ok()?;

    let candidates
        = [format!("python{}.{}", requested.major, requested.minor), format!("python{}", requested.major), "python".to_string()];

    for directory in std::env::split_paths(&path) {
        // Never pick the shims Yarn itself injects in the PATH
        if directory.to_string_lossy().contains("xfs-") || directory.to_string_lossy().contains("/.yarn/") {
            continue;
        }

        for candidate in &candidates {
            let executable
                = directory.join(candidate);

            if !executable.is_file() {
                continue;
            }

            let Ok(executable) = Path::try_from(executable) else {
                continue;
            };

            if let Some(interpreter) = probe(&executable) {
                if version_matches(requested, &interpreter.version) {
                    return Some(interpreter);
                }
            }
        }
    }

    None
}

pub fn managed_root(config: &Configuration) -> Path {
    config.settings.global_folder.value
        .with_join_str("python")
}

fn managed_executable(installation: &Path) -> Path {
    if cfg!(windows) {
        installation.with_join_str("python.exe")
    } else {
        installation.with_join_str("bin/python3")
    }
}

fn find_managed(config: &Configuration, requested: &PythonVersion) -> Option<Interpreter> {
    let root
        = managed_root(config);

    let mut best: Option<(PythonVersion, Path)>
        = None;

    for entry in root.fs_read_dir().ok()?.flatten() {
        let name
            = entry.file_name().to_string_lossy().to_string();

        let Some(version_str) = name.strip_prefix("cpython-") else {
            continue;
        };

        let Some(version) = PythonVersion::parse(version_str.split('-').next().unwrap_or_default()) else {
            continue;
        };

        if !version_matches(requested, &version) {
            continue;
        }

        let installation
            = root.with_join_str(&name);

        if best.as_ref().map_or(true, |(best_version, _)| version > *best_version) {
            best = Some((version, installation));
        }
    }

    let (version, installation)
        = best?;

    let executable
        = managed_executable(&installation);

    executable.fs_exists().then(|| Interpreter {
        home: executable.dirname().unwrap(),
        executable,
        version,
    })
}

#[derive(Deserialize)]
struct LatestRelease {
    asset_url_prefix: String,
    tag: String,
}

async fn download_managed(config: &Configuration, http_client: &HttpClient, requested: &PythonVersion) -> Result<Interpreter, Error> {
    let platform
        = Platform::current()
            .ok_or_else(|| Error::InvalidResolution("Unsupported platform for managed Python builds".to_string()))?;

    let triple
        = platform.standalone_triple()
            .ok_or_else(|| Error::InvalidResolution("Unsupported platform for managed Python builds".to_string()))?;

    let mirror
        = config.settings.python_download_mirror.value.trim_end_matches('/').to_string();

    // The mirror points at `<...>/releases/download`; the latest release
    // pointer is published on the repository's `latest-release` branch.
    let latest_url
        = std::env::var("YARN_PYTHON_LATEST_RELEASE_URL")
            .unwrap_or_else(|_| "https://raw.githubusercontent.com/astral-sh/python-build-standalone/latest-release/latest-release.json".to_string());

    let latest_bytes
        = http_client.get(&latest_url)?.send_bytes().await?.1;

    let latest: LatestRelease
        = serde_json::from_slice(&latest_bytes)
            .map_err(|err| Error::InvalidResolution(format!("Invalid python-build-standalone release pointer: {}", err)))?;

    let prefix = if mirror.contains("github.com/astral-sh/python-build-standalone") {
        latest.asset_url_prefix.clone()
    } else {
        format!("{}/{}", mirror, latest.tag)
    };

    let sums
        = http_client.get(format!("{}/SHA256SUMS", prefix))?.send_text().await?;

    let suffix
        = format!("-{}-install_only_stripped.tar.gz", triple);

    let mut best: Option<(PythonVersion, String, String)>
        = None;

    for line in sums.lines() {
        let Some((sha, filename)) = line.split_once(char::is_whitespace) else {
            continue;
        };

        let filename
            = filename.trim();

        let Some(rest) = filename.strip_prefix("cpython-") else {
            continue;
        };

        if !filename.ends_with(&suffix) {
            continue;
        }

        let Some(version) = PythonVersion::parse(rest.split('+').next().unwrap_or_default()) else {
            continue;
        };

        // Skip pre-releases (`3.15.0a1`)
        if rest.split('+').next().unwrap_or_default().chars().any(|c| c.is_ascii_alphabetic()) {
            continue;
        }

        if !version_matches(requested, &version) {
            continue;
        }

        if best.as_ref().map_or(true, |(best_version, _, _)| version > *best_version) {
            best = Some((version, filename.to_string(), sha.to_string()));
        }
    }

    let (version, filename, sha)
        = best.ok_or_else(|| Error::InvalidResolution(format!("No python-build-standalone build matches Python {}", requested.short())))?;

    let (_, bytes)
        = http_client.get(format!("{}/{}", prefix, filename))?.send_bytes().await?;

    let digest
        = {
            use sha2::Digest;
            hex::encode(sha2::Sha256::digest(&bytes))
        };

    if digest != sha {
        return Err(Error::InvalidResolution(format!("Checksum mismatch for {}", filename)));
    }

    let root
        = managed_root(config);

    let installation
        = root.with_join_str(format!("cpython-{}-{}", version.full(), triple));

    let tmp
        = root.with_join_str(format!(".tmp-{}-{}", std::process::id(), version.full()));

    let _ = tmp.fs_rm();
    tmp.fs_create_dir_all()?;

    let archive_path
        = tmp.with_join_str("python.tar.gz");

    archive_path.fs_write(&bytes)?;

    extract_tar_preserving_links(&archive_path, &tmp)?;

    let extracted
        = tmp.with_join_str("python");

    if extracted.fs_rename(&installation).is_err() && !installation.fs_exists() {
        return Err(Error::InvalidResolution(format!("Failed to install {}", filename)));
    }

    let _ = tmp.fs_rm();

    let executable
        = managed_executable(&installation);

    Ok(Interpreter {
        home: executable.dirname().unwrap(),
        executable,
        version,
    })
}

/// Extracts the build with the system `tar`, which preserves the symlinks
/// and permissions of the distribution (our own extractor only handles the
/// regular files package archives contain).
fn extract_tar_preserving_links(archive: &Path, destination: &Path) -> Result<(), Error> {
    let status
        = Command::new("tar")
            .arg("-xzf")
            .arg(archive.to_path_buf())
            .arg("-C")
            .arg(destination.to_path_buf())
            .status()
            .map_err(|err| Error::InvalidResolution(format!("Failed to run tar: {}", err)))?;

    if !status.success() {
        return Err(Error::InvalidResolution("Failed to extract the Python build".to_string()));
    }

    Ok(())
}

type InterpreterCell = Arc<OnceCell<Interpreter>>;

static INTERPRETERS: LazyLock<DashMap<String, InterpreterCell>>
    = LazyLock::new(DashMap::new);

/// Finds or provisions an interpreter for the requested version.
pub async fn ensure_interpreter(config: &Configuration, http_client: &HttpClient, version: &str) -> Result<Interpreter, Error> {
    let requested
        = PythonVersion::parse(version)
            .ok_or_else(|| Error::InvalidResolution(format!("Invalid Python version: {}", version)))?;

    let cell
        = INTERPRETERS.entry(version.to_string())
            .or_default()
            .clone();

    let interpreter = cell.get_or_try_init(|| async {
        if let Ok(executable) = std::env::var("YARN_PYTHON_EXECUTABLE") {
            if let Some(interpreter) = Path::from_file_string(&executable).ok().as_ref().and_then(probe) {
                return Ok(interpreter);
            }
        }

        let prefer_managed
            = config.settings.python_preference.value == PythonPreference::Managed;

        if prefer_managed {
            if let Some(interpreter) = find_managed(config, &requested) {
                return Ok(interpreter);
            }
        }

        if let Some(interpreter) = find_on_path(&requested) {
            return Ok(interpreter);
        }

        if !prefer_managed {
            if let Some(interpreter) = find_managed(config, &requested) {
                return Ok(interpreter);
            }
        }

        if !config.settings.python_downloads.value {
            return Err(Error::InvalidResolution(format!(
                "No Python {} interpreter found on the system, and pythonDownloads is disabled",
                requested.short(),
            )));
        }

        download_managed(config, http_client, &requested).await
    }).await?;

    Ok(interpreter.clone())
}

pub fn interpreter_display(interpreter: &Interpreter) -> String {
    format!("{} ({})", interpreter.executable.to_file_string(), interpreter.version.full())
}
