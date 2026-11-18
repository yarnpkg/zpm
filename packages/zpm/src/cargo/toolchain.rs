//! Locates (or provisions) the Rust toolchain used by cargo islands.
//!
//! Which toolchain a workspace wants is read from its `rust-toolchain.toml`
//! (the file rustup reads), then from the `rustToolchain` setting, and
//! defaults to `stable`. Lookup order, depending on `rustPreference`:
//!
//! - `managed` (default): a toolchain already present in
//!   `<globalFolder>/rust`, then a matching one on the PATH, then a download
//!   (when `rustDownloads` is enabled).
//! - `system`: a matching toolchain on the PATH first.
//!
//! Managed toolchains are assembled from the official distribution: the
//! channel manifest lists an archive per component and target, with its
//! checksum. They're shared by every project on the machine.

use std::{collections::{BTreeMap, BTreeSet}, process::Command, sync::{Arc, LazyLock}, time::Duration};

use dashmap::DashMap;
use sha2::Digest;
use tokio::sync::OnceCell;
use toml_edit::{DocumentMut, TableLike};
use zpm_config::{Configuration, RustPreference};
use zpm_utils::{Path, ToFileString};

use crate::{
    error::Error,
    http::HttpClient,
    project::Project,
    python_env::Platform,
};

use super::{RustVersion, parse_rust_version};

const TOOLCHAIN_FILES: [&str; 2]
    = ["rust-toolchain.toml", "rust-toolchain"];

/// Lists the components a managed toolchain holds; only written once the
/// toolchain is complete.
const COMPONENTS_MARKER: &str
    = ".yarn-components";

/// What a toolchain can't do without.
const BASE_COMPONENTS: [&str; 3]
    = ["rustc", "cargo", "rust-std"];

/// Toolchain archives are large; the regular timeout is sized for packages.
const DOWNLOAD_TIMEOUT: Duration
    = Duration::from_secs(900);

/// What a workspace asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolchainSpec {
    /// `stable`, `beta`, `nightly`, a version (`1.93.0`, `1.93`), or a
    /// dated channel (`nightly-2026-01-01`)
    pub channel: String,

    /// Components wanted on top of the compiler, the standard library and
    /// Cargo (`clippy`, `rustfmt`, `rust-src`, ...)
    pub components: Vec<String>,

    /// Targets to install the standard library for, besides the host
    pub targets: Vec<String>,
}

impl ToolchainSpec {
    pub fn channel(channel: &str) -> ToolchainSpec {
        ToolchainSpec {
            channel: channel.to_string(),
            components: vec![],
            targets: vec![],
        }
    }
}

fn strings(table: &dyn TableLike, key: &str) -> Vec<String> {
    table.get(key)
        .and_then(|item| item.as_array())
        .map(|array| array.iter().filter_map(|value| value.as_str()).map(|value| value.to_string()).collect())
        .unwrap_or_default()
}

/// Parses a `rust-toolchain.toml`, or a legacy `rust-toolchain` file which
/// only holds the name of a channel.
pub fn parse_toolchain_file(text: &str) -> Option<ToolchainSpec> {
    let toolchain = text.parse::<DocumentMut>().ok()
        .and_then(|document| document.get("toolchain")?.as_table_like().map(|table| {
            let mut components
                = strings(table, "components");

            // rustup's default profile comes with those two
            if table.get("profile").and_then(|item| item.as_str()) == Some("default") {
                components.extend(["rustfmt".to_string(), "clippy".to_string()]);
            }

            ToolchainSpec {
                channel: table.get("channel").and_then(|item| item.as_str()).unwrap_or("stable").to_string(),
                components,
                targets: strings(table, "targets"),
            }
        }));

    if toolchain.is_some() {
        return toolchain;
    }

    let channel
        = text.trim();

    let is_channel
        = !channel.is_empty() && channel.lines().count() == 1 && !channel.contains(['[', '=', ' ']);

    is_channel.then(|| ToolchainSpec::channel(channel))
}

/// Looks for the toolchain file applying to a folder: its own, or the one
/// of the closest parent that has one (which is what rustup does).
pub fn find_toolchain_file(dir: &Path) -> Option<ToolchainSpec> {
    let mut current
        = Some(dir.clone());

    while let Some(dir) = current {
        for name in TOOLCHAIN_FILES {
            if let Ok(text) = dir.with_join_str(name).fs_read_text() {
                return parse_toolchain_file(&text);
            }
        }

        current = dir.dirname().filter(|parent| parent != &dir);
    }

    None
}

pub fn toolchain_spec(config: &Configuration, dir: &Path) -> ToolchainSpec {
    find_toolchain_file(dir)
        .or_else(|| config.settings.rust_toolchain.value.as_deref().map(ToolchainSpec::channel))
        .unwrap_or_else(|| ToolchainSpec::channel("stable"))
}

/// The Rust version a project pins through its toolchain file, when the
/// channel is a version rather than a moving target.
pub fn pinned_rust_version(dir: &Path) -> Option<RustVersion> {
    let spec
        = find_toolchain_file(dir)?;

    spec.channel.starts_with(|c: char| c.is_ascii_digit())
        .then(|| parse_rust_version(&spec.channel))
        .flatten()
}

#[derive(Clone, Debug)]
pub struct Toolchain {
    /// The folder holding `cargo` and `rustc`
    pub bin_dir: Path,

    /// What `rustc --version` reports (`1.93.0`, `1.95.0-nightly`)
    pub version: String,

    pub is_managed: bool,
}

impl Toolchain {
    pub fn binary(&self, name: &str) -> Path {
        match cfg!(windows) {
            true => self.bin_dir.with_join_str(format!("{}.exe", name)),
            false => self.bin_dir.with_join_str(name),
        }
    }
}

/// Whether a toolchain reporting `version` is what `channel` asks for.
/// Dated channels are never satisfied by a toolchain we didn't install,
/// as compilers don't report the date they were published on.
fn version_matches(channel: &str, version: &str) -> bool {
    let is_release
        = !version.contains('-');

    match channel {
        "stable" => is_release,
        "beta" => version.contains("-beta"),
        "nightly" => version.contains("-nightly"),

        _ if channel.starts_with(|c: char| c.is_ascii_digit()) => {
            is_release && (version == channel || version.starts_with(&format!("{}.", channel)))
        },

        _ => false,
    }
}

fn host_triple() -> Result<&'static str, Error> {
    Platform::current()
        .and_then(|platform| platform.standalone_triple())
        .ok_or_else(|| Error::CargoError("Yarn doesn't know which Rust toolchain to download for this platform".to_string()))
}

pub fn managed_root(config: &Configuration) -> Path {
    config.settings.global_folder.value.with_join_str("rust")
}

fn installed_components(installation: &Path) -> Option<BTreeSet<String>> {
    let text
        = installation.with_join_str(COMPONENTS_MARKER).fs_read_text().ok()?;

    Some(text.lines().map(|line| line.to_string()).collect())
}

/// The name of a managed toolchain's folder: `<version>-<host>` for
/// releases, `<channel>-<date>-<host>` otherwise.
fn installation_name(version: &str, date: &str, host: &str) -> String {
    let (number, prerelease)
        = version.split_once('-').map_or((version, None), |(number, prerelease)| (number, Some(prerelease)));

    match prerelease {
        None => format!("{}-{}", number, host),
        Some(prerelease) => format!("{}-{}-{}", prerelease.split('.').next().unwrap(), date, host),
    }
}

/// Finds the best managed toolchain satisfying a channel.
fn find_managed(config: &Configuration, channel: &str) -> Option<(Path, String)> {
    let host
        = host_triple().ok()?;

    let root
        = managed_root(config);

    let suffix
        = format!("-{}", host);

    let mut best: Option<(RustVersion, String, Path, String)>
        = None;

    for entry in root.fs_read_dir().ok()?.flatten() {
        let name
            = entry.file_name().to_string_lossy().to_string();

        let Some(label) = name.strip_suffix(&suffix) else {
            continue;
        };

        let installation
            = root.with_join_str(&name);

        if installed_components(&installation).is_none() {
            continue;
        }

        // Releases are named after their version; the other channels
        // after the day they were published on
        let (rank, version, matches) = match parse_rust_version(label).filter(|_| label.starts_with(|c: char| c.is_ascii_digit())) {
            Some(rank) => (rank, label.to_string(), version_matches(channel, label)),
            None => ((0, 0, 0), label.to_string(), label == channel || label.split('-').next() == Some(channel)),
        };

        if !matches {
            continue;
        }

        let is_better
            = best.as_ref().is_none_or(|(best_rank, best_label, _, _)| (rank, label) > (*best_rank, best_label.as_str()));

        if is_better {
            best = Some((rank, label.to_string(), installation, version));
        }
    }

    best.map(|(_, _, installation, version)| (installation, version))
}

fn find_on_path(channel: &str, cwd: &Path) -> Option<Toolchain> {
    let path
        = std::env::var("PATH").ok()?;

    let suffix
        = if cfg!(windows) {".exe"} else {""};

    for directory in std::env::split_paths(&path) {
        let cargo
            = directory.join(format!("cargo{}", suffix));
        let rustc
            = directory.join(format!("rustc{}", suffix));

        if !cargo.is_file() || !rustc.is_file() {
            continue;
        }

        // Run from the workspace, so that a rustup proxy honors the
        // toolchain file the same way we do
        let output = Command::new(&rustc)
            .arg("--version")
            .current_dir(cwd.to_path_buf())
            .output()
            .ok()?;

        if !output.status.success() {
            continue;
        }

        let stdout
            = String::from_utf8_lossy(&output.stdout).to_string();

        let version
            = stdout.split_whitespace().nth(1)?.to_string();

        if !version_matches(channel, &version) {
            continue;
        }

        return Some(Toolchain {
            bin_dir: Path::try_from(directory).ok()?,
            version,
            is_managed: false,
        });
    }

    None
}

/// What a channel manifest says about an archive.
#[derive(Clone, Debug, PartialEq, Eq)]
struct ComponentArchive {
    url: String,
    sha256: String,
}

#[derive(Debug)]
struct ChannelManifest {
    document: DocumentMut,
}

impl ChannelManifest {
    fn parse(text: &str) -> Result<ChannelManifest, Error> {
        let document
            = text.parse::<DocumentMut>()
                .map_err(|err| Error::CargoError(format!("Invalid Rust channel manifest: {}", err)))?;

        Ok(ChannelManifest {document})
    }

    fn date(&self) -> &str {
        self.document.get("date").and_then(|item| item.as_str()).unwrap_or_default()
    }

    /// `1.93.0`, out of `1.93.0 (254b59607 2026-01-19)`
    fn version(&self) -> Option<&str> {
        self.document.get("pkg")?.get("rust")?.get("version")?.as_str()?.split_whitespace().next()
    }

    /// Components are published under names that changed over time
    /// (`clippy` is `clippy-preview`)
    fn package_name<'a>(&'a self, component: &'a str) -> &'a str {
        self.document.get("renames")
            .and_then(|renames| renames.get(component)?.get("to")?.as_str())
            .unwrap_or(component)
    }

    fn archive(&self, component: &str, target: &str) -> Option<ComponentArchive> {
        let targets
            = self.document.get("pkg")?.get(self.package_name(component))?.get("target")?.as_table_like()?;

        // Target independent components (`rust-src`) are listed under `*`
        let entry
            = targets.get(target).or_else(|| targets.get("*"))?.as_table_like()?;

        if entry.get("available").and_then(|item| item.as_bool()) != Some(true) {
            return None;
        }

        Some(ComponentArchive {
            url: entry.get("url")?.as_str()?.to_string(),
            sha256: entry.get("hash")?.as_str()?.to_string(),
        })
    }
}

/// Where the manifest of a channel is published.
fn manifest_url(mirror: &str, channel: &str) -> String {
    let mirror
        = mirror.trim_end_matches('/');

    // `nightly-2026-01-01` and `beta-2026-01-01` are archived by date
    let dated = channel.split_once('-')
        .filter(|(name, date)| matches!(*name, "nightly" | "beta" | "stable") && date.len() == 10);

    match dated {
        Some((name, date)) => format!("{}/dist/{}/channel-rust-{}.toml", mirror, date, name),
        None => format!("{}/dist/channel-rust-{}.toml", mirror, channel),
    }
}

/// Archives hold one folder per component, listed in their `components`
/// file; installing a component means merging its folder in the toolchain.
fn install_archive(archive: &[u8], staging: &Path, installation: &Path) -> Result<(), Error> {
    if staging.fs_symlink_metadata().is_ok() {
        staging.fs_rm()?;
    }

    staging.fs_create_dir_all()?;

    let tar
        = zpm_formats::tar::unpack_tgz(archive)?;

    zpm_formats::tar::extract_tar_to_disk(&tar, staging)?;

    for entry in staging.fs_read_dir()?.flatten() {
        let package_dir
            = staging.with_join_str(entry.file_name().to_string_lossy());

        let Ok(components) = package_dir.with_join_str("components").fs_read_text() else {
            continue;
        };

        for component in components.lines().filter(|line| !line.trim().is_empty()) {
            merge_tree(&package_dir.with_join_str(component.trim()), installation, true)?;
        }
    }

    staging.fs_rm()?;

    Ok(())
}

fn merge_tree(source: &Path, destination: &Path, is_root: bool) -> Result<(), Error> {
    destination.fs_create_dir_all()?;

    for entry in source.fs_read_dir()?.flatten() {
        let name
            = entry.file_name().to_string_lossy().to_string();

        // The list of the files of the component, for rustup's own use
        if is_root && name == "manifest.in" {
            continue;
        }

        let from
            = source.with_join_str(&name);
        let to
            = destination.with_join_str(&name);

        if entry.file_type()?.is_dir() {
            merge_tree(&from, &to, false)?;
        } else {
            from.fs_rename(&to)?;
        }
    }

    Ok(())
}

async fn download_archive(http_client: &HttpClient, archive: &ComponentArchive) -> Result<Vec<u8>, Error> {
    let (_, bytes) = http_client.get(&archive.url)?
        .timeout(DOWNLOAD_TIMEOUT)
        .send_bytes()
        .await?;

    if hex::encode(sha2::Sha256::digest(&bytes)) != archive.sha256 {
        return Err(Error::CargoError(format!("Checksum mismatch for {}", archive.url)));
    }

    Ok(bytes.to_vec())
}

/// The archives a toolchain is made of, by the name they're recorded
/// under in the components marker.
fn wanted_archives(manifest: &ChannelManifest, spec: &ToolchainSpec, host: &str) -> Result<BTreeMap<String, ComponentArchive>, Error> {
    let mut archives
        = BTreeMap::new();

    let components = BASE_COMPONENTS.iter().copied()
        .chain(spec.components.iter().map(|component| component.as_str()));

    for component in components {
        let archive
            = manifest.archive(component, host)
                .ok_or_else(|| Error::CargoError(format!("The {} toolchain has no `{}` component for {}", spec.channel, component, host)))?;

        archives.insert(manifest.package_name(component).to_string(), archive);
    }

    for target in spec.targets.iter().filter(|target| target.as_str() != host) {
        let archive
            = manifest.archive("rust-std", target)
                .ok_or_else(|| Error::CargoError(format!("The {} toolchain has no standard library for {}", spec.channel, target)))?;

        archives.insert(format!("rust-std-{}", target), archive);
    }

    Ok(archives)
}

async fn fetch_manifest(config: &Configuration, http_client: &HttpClient, channel: &str) -> Result<ChannelManifest, Error> {
    let url
        = manifest_url(&config.settings.rust_download_mirror.value, channel);

    let text
        = http_client.get(&url)?.send_text().await
            .map_err(|err| Error::CargoError(format!("Failed to fetch the manifest of the {} toolchain ({}): {}", channel, url, err)))?;

    ChannelManifest::parse(&text)
}

/// Downloads what a managed toolchain is missing; everything, when it
/// doesn't exist yet.
async fn install_managed(config: &Configuration, http_client: &HttpClient, spec: &ToolchainSpec, existing: Option<(Path, String)>) -> Result<Toolchain, Error> {
    let host
        = host_triple()?;

    // An existing toolchain is completed from the manifest of its own
    // version, not from wherever the channel moved to since
    let channel = match &existing {
        Some((_, version)) if version.starts_with(|c: char| c.is_ascii_digit()) => version.clone(),
        _ => spec.channel.clone(),
    };

    let manifest
        = fetch_manifest(config, http_client, &channel).await?;

    let version
        = manifest.version()
            .ok_or_else(|| Error::CargoError(format!("The manifest of the {} toolchain doesn't tell its version", channel)))?
            .to_string();

    let root
        = managed_root(config);

    let installation = match &existing {
        Some((installation, _)) => installation.clone(),
        None => root.with_join_str(installation_name(&version, manifest.date(), host)),
    };

    let mut installed
        = installed_components(&installation).unwrap_or_default();

    let archives
        = wanted_archives(&manifest, spec, host)?;

    let missing = archives.iter()
        .filter(|(name, _)| !installed.contains(*name))
        .collect::<Vec<_>>();

    let downloads
        = futures::future::try_join_all(missing.iter().map(|(_, archive)| download_archive(http_client, archive))).await?;

    let staging
        = root.with_join_str(format!(".staging-{}-{}", std::process::id(), installation.basename().unwrap_or_default()));

    for ((name, _), archive) in missing.iter().zip(downloads) {
        let installation
            = installation.clone();
        let staging
            = staging.clone();

        tokio::task::spawn_blocking(move || install_archive(&archive, &staging, &installation)).await??;

        installed.insert((*name).clone());
    }

    installation.with_join_str(COMPONENTS_MARKER)
        .fs_write_text(installed.iter().map(|name| format!("{}\n", name)).collect::<String>())?;

    Ok(Toolchain {
        bin_dir: installation.with_join_str("bin"),
        version,
        is_managed: true,
    })
}

/// The managed toolchain a workspace would use, if it's installed. Never
/// touches the network, which makes it usable from anywhere.
pub fn find_installed(config: &Configuration, dir: &Path) -> Option<Toolchain> {
    let (installation, version)
        = find_managed(config, &toolchain_spec(config, dir).channel)?;

    Some(Toolchain {
        bin_dir: installation.with_join_str("bin"),
        version,
        is_managed: true,
    })
}

type ToolchainCell = Arc<OnceCell<Option<Toolchain>>>;

static TOOLCHAINS: LazyLock<DashMap<String, ToolchainCell>>
    = LazyLock::new(DashMap::new);

/// Finds or provisions the toolchain of a workspace. Returns `None` when
/// there's none on the machine and Yarn isn't allowed to download one.
pub async fn ensure_toolchain(project: &Project, dir: &Path) -> Result<Option<Toolchain>, Error> {
    let config
        = &project.config;

    let spec
        = toolchain_spec(config, dir);

    let cell = TOOLCHAINS
        .entry(format!("{:?}@{}", spec, dir.to_file_string()))
        .or_default()
        .clone();

    let toolchain = cell.get_or_try_init(|| async {
        let prefer_managed
            = config.settings.rust_preference.value == RustPreference::Managed;

        let managed
            = find_managed(config, &spec.channel);

        // A managed toolchain is only good as-is when it has everything
        // the workspace asks for; otherwise it gets completed below
        let complete_managed = managed.clone().filter(|(installation, _)| {
            let installed
                = installed_components(installation).unwrap_or_default();

            let has_components
                = spec.components.iter().all(|component| installed.iter().any(|name| name == component || name.strip_suffix("-preview") == Some(component.as_str())));

            let has_targets
                = spec.targets.iter().all(|target| host_triple().is_ok_and(|host| host == target) || installed.contains(&format!("rust-std-{}", target)));

            has_components && has_targets
        });

        let as_toolchain = |(installation, version): (Path, String)| Toolchain {
            bin_dir: installation.with_join_str("bin"),
            version,
            is_managed: true,
        };

        if prefer_managed {
            if let Some(managed) = complete_managed.clone() {
                return Ok(Some(as_toolchain(managed)));
            }
        }

        if managed.is_none() || !prefer_managed {
            if let Some(toolchain) = find_on_path(&spec.channel, dir) {
                return Ok(Some(toolchain));
            }
        }

        if let Some(managed) = complete_managed {
            return Ok(Some(as_toolchain(managed)));
        }

        if !config.settings.rust_downloads.value {
            return Ok(None);
        }

        install_managed(config, &project.http_client, &spec, managed).await.map(Some)
    }).await?;

    Ok(toolchain.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST: &str = r#"
manifest-version = "2"
date = "2026-01-22"

[pkg.rust]
version = "1.93.0 (254b59607 2026-01-19)"

[pkg.rustc.target.x86_64-unknown-linux-gnu]
available = true
url = "https://static.rust-lang.org/dist/2026-01-22/rustc-1.93.0-x86_64-unknown-linux-gnu.tar.gz"
hash = "aa"

[pkg.rustc.target.riscv32-unknown-none]
available = false

[pkg.cargo.target.x86_64-unknown-linux-gnu]
available = true
url = "https://static.rust-lang.org/dist/2026-01-22/cargo-1.93.0-x86_64-unknown-linux-gnu.tar.gz"
hash = "bb"

[pkg.rust-std.target.x86_64-unknown-linux-gnu]
available = true
url = "https://static.rust-lang.org/dist/2026-01-22/rust-std-1.93.0-x86_64-unknown-linux-gnu.tar.gz"
hash = "cc"

[pkg.rust-std.target.wasm32-unknown-unknown]
available = true
url = "https://static.rust-lang.org/dist/2026-01-22/rust-std-1.93.0-wasm32-unknown-unknown.tar.gz"
hash = "dd"

[pkg.clippy-preview.target.x86_64-unknown-linux-gnu]
available = true
url = "https://static.rust-lang.org/dist/2026-01-22/clippy-1.93.0-x86_64-unknown-linux-gnu.tar.gz"
hash = "ee"

[pkg.rust-src.target."*"]
available = true
url = "https://static.rust-lang.org/dist/2026-01-22/rust-src-1.93.0.tar.gz"
hash = "ff"

[renames.clippy]
to = "clippy-preview"
"#;

    const HOST: &str
        = "x86_64-unknown-linux-gnu";

    #[test]
    fn test_toolchain_files() {
        let spec
            = parse_toolchain_file("[toolchain]\nchannel = \"1.93.0\"\ncomponents = [\"clippy\"]\ntargets = [\"wasm32-unknown-unknown\"]\n").unwrap();

        assert_eq!(spec, ToolchainSpec {
            channel: "1.93.0".to_string(),
            components: vec!["clippy".to_string()],
            targets: vec!["wasm32-unknown-unknown".to_string()],
        });

        let with_profile
            = parse_toolchain_file("[toolchain]\nchannel = \"stable\"\nprofile = \"default\"\n").unwrap();

        assert_eq!(with_profile.components, vec!["rustfmt", "clippy"]);

        // The legacy format is the name of a channel, and nothing else
        assert_eq!(parse_toolchain_file("nightly-2026-01-01\n").unwrap(), ToolchainSpec::channel("nightly-2026-01-01"));
        assert_eq!(parse_toolchain_file(""), None);
        assert_eq!(parse_toolchain_file("[other]\nkey = 1\n"), None);
    }

    #[test]
    fn test_version_matching() {
        for (channel, version, expected) in [
            ("stable", "1.93.0", true),
            ("stable", "1.95.0-nightly", false),
            ("1.93.0", "1.93.0", true),
            ("1.93.0", "1.93.1", false),
            ("1.93", "1.93.1", true),
            ("1.9", "1.93.1", false),
            ("nightly", "1.95.0-nightly", true),
            ("beta", "1.94.0-beta.3", true),
            ("nightly", "1.93.0", false),
            ("nightly-2026-01-01", "1.95.0-nightly", false),
        ] {
            assert_eq!(version_matches(channel, version), expected, "{} / {}", channel, version);
        }
    }

    #[test]
    fn test_manifest_urls() {
        let mirror
            = "https://static.rust-lang.org/";

        assert_eq!(manifest_url(mirror, "stable"), "https://static.rust-lang.org/dist/channel-rust-stable.toml");
        assert_eq!(manifest_url(mirror, "1.93.0"), "https://static.rust-lang.org/dist/channel-rust-1.93.0.toml");
        assert_eq!(manifest_url(mirror, "nightly-2026-01-01"), "https://static.rust-lang.org/dist/2026-01-01/channel-rust-nightly.toml");
    }

    #[test]
    fn test_installation_names() {
        assert_eq!(installation_name("1.93.0", "2026-01-22", HOST), "1.93.0-x86_64-unknown-linux-gnu");
        assert_eq!(installation_name("1.95.0-nightly", "2026-01-22", HOST), "nightly-2026-01-22-x86_64-unknown-linux-gnu");
        assert_eq!(installation_name("1.94.0-beta.3", "2026-01-22", HOST), "beta-2026-01-22-x86_64-unknown-linux-gnu");
    }

    #[test]
    fn test_archives() {
        let manifest
            = ChannelManifest::parse(MANIFEST).unwrap();

        assert_eq!(manifest.version(), Some("1.93.0"));
        assert_eq!(manifest.date(), "2026-01-22");

        let spec = ToolchainSpec {
            channel: "1.93.0".to_string(),
            components: vec!["clippy".to_string(), "rust-src".to_string()],
            targets: vec!["wasm32-unknown-unknown".to_string(), HOST.to_string()],
        };

        let archives
            = wanted_archives(&manifest, &spec, HOST).unwrap();

        // Renamed components are recorded under their published name, and
        // the host doesn't need its standard library twice
        assert_eq!(archives.keys().map(|name| name.as_str()).collect::<Vec<_>>(), vec![
            "cargo",
            "clippy-preview",
            "rust-src",
            "rust-std",
            "rust-std-wasm32-unknown-unknown",
            "rustc",
        ]);

        assert_eq!(archives["rust-src"].sha256, "ff");
        assert_eq!(archives["rust-std-wasm32-unknown-unknown"].sha256, "dd");

        let unavailable
            = wanted_archives(&manifest, &ToolchainSpec::channel("1.93.0"), "riscv32-unknown-none").unwrap_err();

        assert!(unavailable.to_string().contains("has no `rustc` component"), "{}", unavailable);
    }

    #[test]
    fn test_archive_installation() {
        use zpm_formats::tar::ToTar;

        let root
            = Path::temp_dir().unwrap();

        let entry = |name: &str, data: &'static [u8], mode: u32| zpm_formats::Entry {
            name: Path::try_from(name).unwrap(),
            mode,
            crc: 0,
            data: std::borrow::Cow::Borrowed(data),
            compression: None,
        };

        let archive = vec![
            entry("rustc-1.93.0-host/components", b"rustc\nrust-std-host\n", 0o644),
            entry("rustc-1.93.0-host/install.sh", b"#!/bin/sh\n", 0o755),
            entry("rustc-1.93.0-host/rustc/manifest.in", b"file:bin/rustc\n", 0o644),
            entry("rustc-1.93.0-host/rustc/bin/rustc", b"binary", 0o755),
            entry("rustc-1.93.0-host/rust-std-host/manifest.in", b"file:lib/libstd.rlib\n", 0o644),
            entry("rustc-1.93.0-host/rust-std-host/lib/libstd.rlib", b"library", 0o644),
        ].to_tgz().unwrap();

        let installation
            = root.with_join_str("toolchain");

        install_archive(&archive, &root.with_join_str("staging"), &installation).unwrap();

        assert_eq!(installation.with_join_str("bin/rustc").fs_read_text().unwrap(), "binary");
        assert_eq!(installation.with_join_str("lib/libstd.rlib").fs_read_text().unwrap(), "library");

        // Only the content of the components is installed
        assert!(!installation.with_join_str("manifest.in").fs_exists());
        assert!(!installation.with_join_str("install.sh").fs_exists());
        assert!(!root.with_join_str("staging").fs_exists());
    }
}
