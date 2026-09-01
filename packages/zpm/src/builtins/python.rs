use std::{borrow::Cow, collections::{btree_map::Entry as BTreeMapEntry, BTreeMap}, iter::once, str::FromStr, sync::{Arc, LazyLock}};

use dashmap::DashMap;
use itertools::Itertools;
use tokio::sync::OnceCell;
use serde::{Deserialize, Serialize};
use zpm_formats::Entry;
use zpm_parsers::JsonDocument;
use zpm_primitives::{BuiltinRange, BuiltinReference, Descriptor, Ident, Locator};
use zpm_utils::{Cpu, FromFileString, Libc, Os, Path, RawPath, Sha256, System, ToFileString};

use crate::{
    error::Error, fetchers::PackageData, install::{FetchResult, InstallContext, IntoResolutionResult, ResolutionResult}, manifest::bin::BinField, npm::NpmEntryExt, resolvers::Resolution
};

/// Ident of the selector builtin that resolves to per-platform variants
pub const PYTHON_IDENT: &str = "@yarnpkg/python";

/// Prefix shared by every platform-variant ident (`@yarnpkg/python-<platform>`)
pub const PYTHON_VARIANT_IDENT_PREFIX: &str = "@yarnpkg/python-";

/// True for any managed-Python builtin ident, selector or platform variant.
/// This module owns the managed-Python naming protocol: dispatch on these
/// idents must go through these predicates, never through ad-hoc string
/// comparisons.
pub fn is_python_ident(ident: &Ident) -> bool {
    ident.as_str() == PYTHON_IDENT || is_python_variant_ident(ident)
}

/// True for platform-variant idents only (`@yarnpkg/python-<platform>`)
pub fn is_python_variant_ident(ident: &Ident) -> bool {
    ident.as_str().starts_with(PYTHON_VARIANT_IDENT_PREFIX)
}

fn python_variant_ident(variant: &PythonPlatformVariant) -> String {
    format!("{PYTHON_VARIANT_IDENT_PREFIX}{}", variant.file_name)
}

static PLATFORM_VARIANTS: &[PythonPlatformVariant] = &[
    PythonPlatformVariant {
        system: System::new(Some(Cpu::X86_64), Some(Os::Linux), Some(Libc::Glibc)),
        file_name: "linux-x64-glibc",
        standalone_target: "x86_64-unknown-linux-gnu",
        metadata_arch: "x86_64",
        metadata_os: "linux",
        metadata_libc: "gnu",
    },
    PythonPlatformVariant {
        system: System::new(Some(Cpu::Aarch64), Some(Os::Linux), Some(Libc::Glibc)),
        file_name: "linux-arm64-glibc",
        standalone_target: "aarch64-unknown-linux-gnu",
        metadata_arch: "aarch64",
        metadata_os: "linux",
        metadata_libc: "gnu",
    },
    PythonPlatformVariant {
        system: System::new(Some(Cpu::X86_64), Some(Os::Linux), Some(Libc::Musl)),
        file_name: "linux-x64-musl",
        standalone_target: "x86_64-unknown-linux-musl",
        metadata_arch: "x86_64",
        metadata_os: "linux",
        metadata_libc: "musl",
    },
    PythonPlatformVariant {
        system: System::new(Some(Cpu::Aarch64), Some(Os::Linux), Some(Libc::Musl)),
        file_name: "linux-arm64-musl",
        standalone_target: "aarch64-unknown-linux-musl",
        metadata_arch: "aarch64",
        metadata_os: "linux",
        metadata_libc: "musl",
    },
    PythonPlatformVariant {
        system: System::new(Some(Cpu::X86_64), Some(Os::MacOS), None),
        file_name: "darwin-x64",
        standalone_target: "x86_64-apple-darwin",
        metadata_arch: "x86_64",
        metadata_os: "darwin",
        metadata_libc: "none",
    },
    PythonPlatformVariant {
        system: System::new(Some(Cpu::Aarch64), Some(Os::MacOS), None),
        file_name: "darwin-arm64",
        standalone_target: "aarch64-apple-darwin",
        metadata_arch: "aarch64",
        metadata_os: "darwin",
        metadata_libc: "none",
    },
];

const CPYTHON_DOWNLOADS_URL_PREFIX: &str = "https://github.com/astral-sh/python-build-standalone/releases/download";

struct PythonPlatformVariant {
    system: System,
    file_name: &'static str,
    standalone_target: &'static str,
    metadata_arch: &'static str,
    metadata_os: &'static str,
    metadata_libc: &'static str,
}

#[derive(Clone, Debug)]
struct PythonReleaseManifest {
    version: zpm_semver::Version,
    build: Option<String>,
    files: BTreeMap<String, PythonReleaseFile>,
}

#[derive(Clone, Debug)]
struct PythonReleaseFile {
    url: String,
    sha256: Option<String>,
    build: Option<String>,
    priority: PythonReleasePriority,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct PythonReleasePriority {
    flavor: usize,
    build_options: usize,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacyPythonReleaseManifest {
    version: zpm_semver::Version,

    #[serde(default)]
    build: Option<String>,

    #[serde(default)]
    files: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct JsonPythonDownload {
    name: String,
    arch: JsonPythonArch,
    os: String,
    libc: String,
    major: u32,
    minor: u32,
    patch: u32,
    prerelease: Option<String>,
    url: String,
    sha256: Option<String>,
    variant: Option<String>,
    build: Option<String>,
}

#[derive(Debug, Deserialize)]
struct JsonPythonArch {
    family: String,
    variant: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StandaloneReleaseRecord {
    version: String,
    artifacts: Vec<StandaloneReleaseArtifact>,
}

#[derive(Debug, Deserialize)]
struct StandaloneReleaseArtifact {
    url: String,
    sha256: Option<String>,
    platform: String,
    variant: String,
}

/// Parsed release indexes, memoized by URL: the HTTP bytes are already
/// cached, but the index is large (astral's full python-build-standalone
/// NDJSON) and requested once per descriptor/locator/variant resolution,
/// so re-parsing it each time is wasteful. Same coalescing shape as
/// `http_npm::METADATA_CACHE`: concurrent callers (e.g. Python target
/// forks resolving in parallel) share a single parse, and errors aren't
/// cached.
type PythonReleasesCell = Arc<OnceCell<Arc<Vec<PythonReleaseManifest>>>>;

static PYTHON_RELEASES_CACHE: LazyLock<DashMap<String, PythonReleasesCell>>
    = LazyLock::new(DashMap::new);

async fn fetch_python_releases(context: &InstallContext<'_>) -> Result<Arc<Vec<PythonReleaseManifest>>, Error> {
    let project = context.project
        .expect("The project is required for resolving a Python package");

    let release_url
        = &project.config.settings.python_dist_metadata_url.value;

    let cell
        = PYTHON_RELEASES_CACHE
            .entry(release_url.clone())
            .or_default()
            .clone();

    let releases = cell.get_or_try_init(|| async {
        let bytes
            = project.http_client.cached_get(release_url).await?;
        let text
            = String::from_utf8_lossy(&bytes);

        parse_python_releases(&text).map(Arc::new)
    }).await?;

    Ok(releases.clone())
}

fn parse_python_releases(text: &str) -> Result<Vec<PythonReleaseManifest>, Error> {
    if let Ok(releases) = JsonDocument::hydrate_from_str::<Vec<LegacyPythonReleaseManifest>>(text) {
        return Ok(releases.into_iter().map(|release| {
            PythonReleaseManifest {
                version: release.version,
                build: release.build,
                files: release.files.into_iter().map(|(platform, url)| {
                    (platform, PythonReleaseFile {
                        url,
                        sha256: None,
                        build: None,
                        priority: PythonReleasePriority {
                            flavor: 0,
                            build_options: 0,
                        },
                    })
                }).collect(),
            }
        }).collect());
    }

    if let Ok(downloads) = JsonDocument::hydrate_from_str::<BTreeMap<String, JsonPythonDownload>>(text) {
        let releases
            = parse_json_python_downloads(downloads);

        if !releases.is_empty() {
            return Ok(releases);
        }
    }

    parse_standalone_release_records(text)
}

fn parse_json_python_downloads(downloads: BTreeMap<String, JsonPythonDownload>) -> Vec<PythonReleaseManifest> {
    let mut releases
        = BTreeMap::new();

    for download in downloads.into_values() {
        if download.name != "cpython" || download.prerelease.is_some() || download.variant.is_some() || download.arch.variant.is_some() {
            continue;
        }

        let Some(variant)
            = variant_from_metadata(&download.os, &download.arch.family, &download.libc) else {
                continue;
            };

        let version
            = zpm_semver::Version::new_from_components(download.major, download.minor, download.patch, None);

        insert_python_release_file(
            &mut releases,
            version,
            variant,
            PythonReleaseFile {
                url: download.url,
                sha256: download.sha256,
                build: download.build,
                priority: PythonReleasePriority {
                    flavor: 0,
                    build_options: 0,
                },
            },
        );
    }

    releases.into_values().collect()
}

fn parse_standalone_release_records(text: &str) -> Result<Vec<PythonReleaseManifest>, Error> {
    let mut releases
        = BTreeMap::new();
    let mut parsed_any
        = false;

    for line in text.lines() {
        let line
            = line.trim();

        if line.is_empty() {
            continue;
        }

        parsed_any = true;

        let record
            = JsonDocument::hydrate_from_str::<StandaloneReleaseRecord>(line)?;
        let Some((version, build)) = parse_standalone_version(&record.version) else {
            continue;
        };

        for artifact in record.artifacts {
            let Some(priority)
                = standalone_artifact_priority(&artifact.variant) else {
                    continue;
                };

            let Some(variant)
                = variant_from_standalone_platform(&artifact.platform) else {
                    continue;
                };

            insert_python_release_file(
                &mut releases,
                version.clone(),
                variant,
                PythonReleaseFile {
                    url: artifact.url,
                    sha256: artifact.sha256,
                    build: build.clone(),
                    priority,
                },
            );
        }
    }

    if parsed_any && !releases.is_empty() {
        return Ok(releases.into_values().collect());
    }

    Err(Error::InvalidResolution(
        "No supported CPython downloads found in Python distribution metadata".to_string(),
    ))
}

fn parse_standalone_version(version: &str) -> Option<(zpm_semver::Version, Option<String>)> {
    let (version, build)
        = version.split_once('+')
            .map(|(version, build)| (version, Some(build.to_string())))
            .unwrap_or((version, None));

    let version
        = zpm_semver::Version::from_file_string(version).ok()?;

    if version.rc.is_some() {
        return None;
    }

    Some((version, build))
}

fn standalone_artifact_priority(variant: &str) -> Option<PythonReleasePriority> {
    let known_flavors
        = ["full", "install_only", "install_only_stripped"];
    let flavor_preferences
        = ["install_only_stripped", "install_only", "shared-pgo", "shared-noopt", "static-noopt", "full"];

    let mut parts
        = variant.split('+').collect_vec();
    let flavor
        = if parts.last().is_some_and(|part| known_flavors.contains(part)) {
            parts.pop().unwrap()
        } else {
            variant
        };

    if flavor.contains("static") || parts.iter().any(|part| matches!(*part, "debug" | "freethreaded") || part.contains("static")) {
        return None;
    }

    Some(PythonReleasePriority {
        flavor: flavor_preferences.iter()
            .position(|candidate| candidate == &flavor)
            .unwrap_or(flavor_preferences.len()),
        build_options: parts.len(),
    })
}

fn variant_from_metadata(os: &str, arch: &str, libc: &str) -> Option<&'static PythonPlatformVariant> {
    PLATFORM_VARIANTS.iter().find(|variant| {
        variant.metadata_os == os
            && variant.metadata_arch == arch
            && variant.metadata_libc == libc
    })
}

fn variant_from_standalone_platform(platform: &str) -> Option<&'static PythonPlatformVariant> {
    let platform
        = platform.strip_suffix("-debug").unwrap_or(platform);
    let platform
        = platform.strip_suffix("-freethreaded").unwrap_or(platform);

    let pieces
        = platform.split('-').collect_vec();

    let arch
        = pieces.first()?;
    let os
        = pieces.get(2)?;
    let libc
        = if *os == "linux" {
            pieces.get(3).copied().unwrap_or("gnu")
        } else {
            "none"
        };

    variant_from_metadata(os, arch, libc)
}

fn insert_python_release_file(
    releases: &mut BTreeMap<zpm_semver::Version, PythonReleaseManifest>,
    version: zpm_semver::Version,
    variant: &PythonPlatformVariant,
    file: PythonReleaseFile,
) {
    let release
        = releases.entry(version.clone()).or_insert_with(|| PythonReleaseManifest {
            version,
            build: None,
            files: BTreeMap::new(),
        });

    if release.build.as_ref() < file.build.as_ref() {
        release.build = file.build.clone();
    }

    match release.files.entry(variant.file_name.to_string()) {
        BTreeMapEntry::Vacant(entry) => {
            entry.insert(file);
        },

        BTreeMapEntry::Occupied(mut entry) => {
            let current
                = entry.get();

            if file.priority < current.priority
                || (file.priority == current.priority && file.build.as_ref() > current.build.as_ref()) {
                entry.insert(file);
            }
        },
    }
}

fn python_release_file<'a>(
    release: Option<&'a PythonReleaseManifest>,
    variant: &PythonPlatformVariant,
) -> Option<&'a PythonReleaseFile> {
    release.and_then(|release| release.files.get(variant.file_name))
}

pub async fn resolve_python_version(context: &InstallContext<'_>, range: &zpm_semver::Range) -> Result<Option<zpm_semver::Version>, Error> {
    if let Some(version) = range.exact_version() {
        return Ok(Some(version));
    }

    let releases
        = fetch_python_releases(context).await?;

    let highest_matching_version
        = releases.iter()
            .filter(|release| range.check(&release.version))
            .max_by(|a, b| a.version.cmp(&b.version))
            .map(|release| release.version.clone());

    Ok(highest_matching_version)
}

async fn resolve_python_release(context: &InstallContext<'_>, version: &zpm_semver::Version) -> Result<Option<PythonReleaseManifest>, Error> {
    Ok(fetch_python_releases(context).await?
        .iter()
        .filter(|release| &release.version == version)
        .max_by(|a, b| a.build.cmp(&b.build))
        .cloned())
}

pub async fn resolve_python_descriptor(context: &InstallContext<'_>, descriptor: &Descriptor, params: &BuiltinRange) -> Result<ResolutionResult, Error> {
    let version
        = resolve_python_version(context, &params.range).await?
            .ok_or(Error::NoCandidatesFound(descriptor.range.clone()))?;

    let locator = descriptor.resolve_with(BuiltinReference {
        version: version.clone(),
    }.into());

    build_python_parent_resolution(context, locator, version)
}

pub async fn resolve_python_locator(context: &InstallContext<'_>, locator: &Locator, version: &zpm_semver::Version) -> Result<ResolutionResult, Error> {
    build_python_parent_resolution(context, locator.clone(), version.clone())
}

fn build_python_parent_resolution(context: &InstallContext<'_>, locator: Locator, version: zpm_semver::Version) -> Result<ResolutionResult, Error> {
    let variants = PLATFORM_VARIANTS.iter().map(|variant| {
        let name
            = python_variant_ident(variant);
        let range
            = zpm_semver::Range::exact(version.clone());

        Descriptor::new(Ident::new(name), BuiltinRange {range}.into())
    }).collect_vec();

    let mut resolution
        = Resolution::new_empty(locator, version);

    resolution.variants = variants;

    let mut resolution_result
        = resolution.into_resolution_result(context)?;

    resolution_result.package_data = Some(PackageData::Abstract);

    Ok(resolution_result)
}

pub async fn resolve_python_variant_descriptor(context: &InstallContext<'_>, descriptor: &Descriptor, range: &zpm_semver::Range) -> Result<ResolutionResult, Error> {
    let variant
        = PLATFORM_VARIANTS.iter()
            .find(|variant| descriptor.ident.as_str() == &python_variant_ident(variant))
            .ok_or(Error::Unsupported)?;

    let version
        = resolve_python_version(context, range).await?
            .ok_or(Error::NoCandidatesFound(descriptor.range.clone()))?;

    let locator = descriptor.resolve_with(BuiltinReference {
        version: version.clone(),
    }.into());

    build_python_variant_resolution(context, locator, version, variant)
}

pub async fn resolve_python_variant_locator(context: &InstallContext<'_>, locator: &Locator, version: &zpm_semver::Version) -> Result<ResolutionResult, Error> {
    let variant
        = PLATFORM_VARIANTS.iter()
            .find(|variant| locator.ident.as_str() == &python_variant_ident(variant))
            .ok_or(Error::Unsupported)?;

    build_python_variant_resolution(context, locator.clone(), version.clone(), variant)
}

fn build_python_variant_resolution(context: &InstallContext<'_>, locator: Locator, version: zpm_semver::Version, variant: &PythonPlatformVariant) -> Result<ResolutionResult, Error> {
    let mut resolution
        = Resolution::new_empty(locator, version);

    resolution.requirements = variant.system.to_requirements();

    resolution.into_resolution_result(context)
}

fn python_release_url(
    context: &InstallContext<'_>,
    release: Option<&PythonReleaseManifest>,
    version: &zpm_semver::Version,
    variant: &PythonPlatformVariant,
) -> String {
    if let Some(file) = python_release_file(release, variant) {
        return rewrite_python_download_url(context, &file.url);
    }

    let project = context.project
        .expect("The project is required for fetching a Python package");

    let version_str
        = version.to_file_string();

    if let Some(build) = release.and_then(|release| release.build.as_ref()) {
        return format!(
            "{}/{}/cpython-{}%2B{}-{}-install_only_stripped.tar.gz",
            project.config.settings.python_dist_url.value.trim_end_matches('/'),
            build,
            version_str,
            build,
            variant.standalone_target,
        );
    }

    format!(
        "{}/v{}/python-v{}-{}.tar.gz",
        project.config.settings.python_dist_url.value.trim_end_matches('/'),
        version_str,
        version_str,
        variant.file_name,
    )
}

fn rewrite_python_download_url(context: &InstallContext<'_>, url: &str) -> String {
    let project = context.project
        .expect("The project is required for fetching a Python package");

    let Some(suffix) = url.strip_prefix(CPYTHON_DOWNLOADS_URL_PREFIX) else {
        return url.to_string();
    };

    format!(
        "{}/{}",
        project.config.settings.python_dist_url.value.trim_end_matches('/'),
        suffix.trim_start_matches('/'),
    )
}

fn python_bin_name(version: &zpm_semver::Version) -> String {
    format!("python{}.{}", version.major, version.minor)
}

fn strip_python_distribution_prefix(mut entry: Entry<'_>) -> Option<Entry<'_>> {
    for prefix in [
        Path::from_str("python/install").unwrap(),
        Path::from_str("install").unwrap(),
    ] {
        if let Some(stripped) = entry.name.strip_prefix(&prefix) {
            entry.name = stripped;
            return Some(entry);
        }
    }

    entry.name.strip_first_segment().map(|stripped| {
        entry.name = stripped;
        entry
    })
}

fn find_python_executable(entries: &[Entry<'_>], version: &zpm_semver::Version) -> Result<Path, Error> {
    let versioned_bin
        = format!("bin/{}", python_bin_name(version));

    for candidate in [
        versioned_bin.as_str(),
        "bin/python3",
        "bin/python",
    ] {
        let candidate_path
            = Path::from_str(candidate).unwrap();

        if entries.iter().any(|entry| entry.name == candidate_path) {
            return Ok(candidate_path);
        }
    }

    Err(Error::InvalidResolution(format!(
        "The Python distribution for {} doesn't contain a supported Python executable",
        version.to_file_string(),
    )))
}

fn make_python_shim(target: &Path) -> Result<Entry<'static>, Error> {
    let target
        = target.to_file_string();

    let script
        = format!("#!/bin/sh\nHERE=$(CDPATH= cd -- \"$(dirname -- \"$0\")\" && pwd)\nexec \"$HERE/{}\" \"$@\"\n", target.strip_prefix("bin/").unwrap_or(&target));

    let mut entry
        = Entry::new_file(Path::from_str("bin/python").unwrap(), Cow::Owned(script.into_bytes()));

    entry.mode = 0o755;

    Ok(entry)
}

pub async fn fetch_python_locator<'a>(context: &InstallContext<'a>, locator: &Locator, version: &zpm_semver::Version, is_mock_request: bool) -> Result<FetchResult, Error> {
    let variant
        = PLATFORM_VARIANTS.iter()
            .find(|variant| locator.ident.as_str() == &python_variant_ident(variant))
            .ok_or(Error::Unsupported)?;

    if is_mock_request {
        let archive_path = context.package_cache.unwrap()
            .key_path(locator, ".zip");

        let package_directory = archive_path
            .with_join(&locator.ident.nm_subdir());

        return Ok(FetchResult::new_mock(archive_path, package_directory));
    }

    let release
        = resolve_python_release(context, version).await?;

    let url
        = python_release_url(context, release.as_ref(), version, variant);
    let expected_sha256
        = python_release_file(release.as_ref(), variant)
            .and_then(|file| file.sha256.clone());

    let project = context.project
        .expect("The project is required for fetching a Python package");

    let package_cache = context.package_cache
        .expect("The package cache is required for fetching Python packages");
    let cache_packer
        = package_cache.packer();

    let package_subdir
        = locator.ident.nm_subdir();
    let package_subdir_for_entries
        = package_subdir.clone();
    let locator_ident
        = locator.ident.clone();
    let version_str
        = version.to_file_string();
    let system_os
        = variant.system.os.clone();
    let system_arch
        = variant.system.arch.clone();
    let system_libc
        = variant.system.libc.clone();
    let version_for_archive
        = version.clone();

    let cached_blob = package_cache.ensure_blob(locator.clone(), ".zip", || async move {
        let bytes
            = project.http_client.get(&url)?
                .send().await?
                .error_for_status()?
                .bytes().await?;

        if let Some(expected_sha256) = &expected_sha256 {
            let actual_sha256
                = Sha256::new(&bytes).to_hex();

            if !actual_sha256.eq_ignore_ascii_case(expected_sha256) {
                return Err(Error::InvalidResolution(format!(
                    "Checksum mismatch for Python distribution {}",
                    version_str,
                )));
            }
        }

        let archive = tokio::task::spawn_blocking(move || -> Result<Vec<u8>, Error> {
            let tar_data
                = zpm_formats::tar::unpack_tgz(&bytes)?;

            #[derive(Serialize)]
            #[serde(rename_all = "camelCase")]
            struct GeneratedManifest<'a> {
                name: &'a str,
                version: &'a str,
                os: &'a Os,
                cpu: &'a Cpu,
                #[serde(skip_serializing_if = "Option::is_none")]
                libc: Option<&'a Libc>,
                prefer_unplugged: bool,
                bin: BinField,
            }

            let mut entries
                = zpm_formats::tar::entries_from_tar(&tar_data)?
                    .into_iter()
                    .filter_map(strip_python_distribution_prefix)
                    .collect::<Vec<_>>();

            let python_executable
                = find_python_executable(&entries, &version_for_archive)?;

            if python_executable != Path::from_str("bin/python").unwrap() {
                entries.push(make_python_shim(&python_executable)?);
            }

            let manifest = GeneratedManifest {
                name: locator_ident.as_str(),
                version: version_str.as_str(),
                os: system_os.as_ref().unwrap(),
                cpu: system_arch.as_ref().unwrap(),
                libc: system_libc.as_ref(),
                prefer_unplugged: true,
                bin: BinField::Map(BTreeMap::from([
                    (Ident::from_str("python").unwrap(), RawPath {
                        raw: "bin/python".to_string(),
                        path: Path::from_str("bin/python").unwrap(),
                    }),
                    (Ident::from_str("python3").unwrap(), RawPath {
                        raw: "bin/python".to_string(),
                        path: Path::from_str("bin/python").unwrap(),
                    }),
                ])),
            };

            let serialized_manifest
                = JsonDocument::to_string(&manifest)?;

            let entries
                = entries.into_iter()
                    .chain(once(Entry::new_file(Path::from_str("package.json").unwrap(), Cow::Owned(serialized_manifest.into_bytes()))))
                    .prepare_npm_entries(&package_subdir_for_entries)?;

            Ok(cache_packer.pack(entries)?)
        }).await??;

        Ok(archive)
    }).await?.into_info();

    let package_directory = cached_blob.path
        .with_join(&package_subdir);

    Ok(FetchResult::new(PackageData::Zip {
        archive_path: cached_blob.path,
        checksum: cached_blob.checksum,
        context_directory: package_directory.clone(),
        package_directory,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find_release<'a>(releases: &'a [PythonReleaseManifest], version: &str) -> &'a PythonReleaseManifest {
        let version
            = zpm_semver::Version::from_file_string(version).unwrap();

        releases.iter()
            .find(|release| release.version == version)
            .unwrap()
    }

    #[test]
    fn parses_python_build_standalone_ndjson_metadata() {
        let metadata
            = r#"{"version":"3.12.4+20240713","artifacts":[{"url":"https://github.com/astral-sh/python-build-standalone/releases/download/20240713/cpython-3.12.4%2B20240713-x86_64-unknown-linux-gnu-install_only.tar.gz","sha256":"install","platform":"x86_64-unknown-linux-gnu","variant":"install_only"},{"url":"https://github.com/astral-sh/python-build-standalone/releases/download/20240713/cpython-3.12.4%2B20240713-x86_64-unknown-linux-gnu-install_only_stripped.tar.gz","sha256":"stripped","platform":"x86_64-unknown-linux-gnu","variant":"install_only_stripped"},{"url":"https://github.com/astral-sh/python-build-standalone/releases/download/20240713/cpython-3.12.4%2B20240713-x86_64-unknown-linux-gnu-debug-install_only_stripped.tar.gz","sha256":"debug","platform":"x86_64-unknown-linux-gnu-debug","variant":"debug+install_only_stripped"}]}"#;

        let releases
            = parse_python_releases(metadata).unwrap();
        let release
            = find_release(&releases, "3.12.4");
        let file
            = release.files.get("linux-x64-glibc").unwrap();

        assert!(file.url.ends_with("install_only_stripped.tar.gz"));
        assert_eq!(file.sha256.as_deref(), Some("stripped"));
        assert_eq!(release.build.as_deref(), Some("20240713"));
    }

    #[test]
    fn parses_uv_json_download_metadata() {
        let metadata
            = r#"{"cpython-3.12.4-darwin-aarch64-none":{"name":"cpython","arch":{"family":"aarch64","variant":null},"os":"darwin","libc":"none","major":3,"minor":12,"patch":4,"prerelease":null,"url":"https://github.com/astral-sh/python-build-standalone/releases/download/20240713/cpython-3.12.4%2B20240713-aarch64-apple-darwin-install_only_stripped.tar.gz","sha256":"abc123","variant":null,"build":"20240713"}}"#;

        let releases
            = parse_python_releases(metadata).unwrap();
        let release
            = find_release(&releases, "3.12.4");
        let file
            = release.files.get("darwin-arm64").unwrap();

        assert_eq!(file.sha256.as_deref(), Some("abc123"));
        assert_eq!(release.build.as_deref(), Some("20240713"));
    }
}
