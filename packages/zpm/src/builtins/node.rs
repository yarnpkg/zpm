use std::{borrow::Cow, collections::BTreeMap, iter::once, str::FromStr};

use itertools::Itertools;
use serde::{Deserialize, Serialize};
use zpm_formats::{Entry, iter_ext::IterExt};
use zpm_parsers::JsonDocument;
use zpm_primitives::{BuiltinRange, BuiltinReference, Descriptor, Ident, Locator};
use zpm_utils::{Cpu, Os, Path, RawPath, System, ToFileString};

use crate::{
    error::Error, fetchers::PackageData, install::{FetchResult, InstallContext, IntoResolutionResult, ResolutionResult}, manifest::bin::BinField, npm::NpmEntryExt, resolvers::Resolution
};

static PLATFORM_VARIANTS: &[(System, &str, &str)] = &[
    (System::new(Some(Cpu::X86_64), Some(Os::Linux), None), "linux-x64", "bin/node"),
    (System::new(Some(Cpu::Aarch64), Some(Os::Linux), None), "linux-arm64", "bin/node"),
    (System::new(Some(Cpu::X86_64), Some(Os::MacOS), None), "darwin-x64", "bin/node"),
    (System::new(Some(Cpu::Aarch64), Some(Os::MacOS), None), "darwin-arm64", "bin/node"),
];

pub async fn resolve_nodejs_version(context: &InstallContext<'_>, range: &zpm_semver::Range) -> Result<Option<zpm_semver::Version>, Error> {
    if let Some(version) = range.exact_version() {
        return Ok(Some(version));
    }

    let project = context.project
        .expect("The project is required for resolving a workspace package");

    let release_url
        = format!("{}/index.json", project.config.settings.node_dist_url.value);

    let text
        = project.http_client.get(&release_url)?.send().await?.text().await?;

    #[derive(Deserialize)]
    struct NodejsManifest {
        version: zpm_semver::Version,
    }

    let releases: Vec<NodejsManifest>
        = JsonDocument::hydrate_from_str(&text)?;

    let highest_matching_version
        = releases.into_iter()
            .filter(|release| range.check(&release.version))
            .max_by(|a, b| a.version.cmp(&b.version))
            .map(|release| release.version);

    Ok(highest_matching_version)
}

pub async fn resolve_nodejs_descriptor(context: &InstallContext<'_>, descriptor: &Descriptor, params: &BuiltinRange) -> Result<ResolutionResult, Error> {
    let version
        = resolve_nodejs_version(context, &params.range).await?
            .ok_or(Error::NoCandidatesFound(descriptor.range.clone()))?;

    let variants = PLATFORM_VARIANTS.iter().map(|(_, file_name, _)| {
        let name
            = format!("@builtin/node-{}", file_name);
        let range
            = zpm_semver::Range::exact(version.clone());

        Descriptor::new(Ident::new(name), BuiltinRange {range}.into())
    }).collect_vec();

    let locator = descriptor.resolve_with(BuiltinReference {
        version: version.clone(),
    }.into());

    let mut resolution
        = Resolution::new_empty(locator, version);

    resolution.variants = variants;

    let mut resolution_result
        = resolution.into_resolution_result(context);

    resolution_result.package_data = Some(PackageData::Abstract);

    Ok(resolution_result)
}

pub async fn resolve_nodejs_variant_descriptor(context: &InstallContext<'_>, descriptor: &Descriptor, range: &zpm_semver::Range) -> Result<ResolutionResult, Error> {
    let (system, _, _)
        = PLATFORM_VARIANTS.iter()
            .find(|(_, file_name, _)| descriptor.ident.as_str() == &format!("@builtin/node-{}", file_name))
            .ok_or(Error::Unsupported)?;

    let version
        = resolve_nodejs_version(context, range).await?
            .ok_or(Error::NoCandidatesFound(descriptor.range.clone()))?;

    let locator = descriptor.resolve_with(BuiltinReference {
        version: version.clone(),
    }.into());

    let mut resolution
        = Resolution::new_empty(locator, version);

    resolution.requirements = system.to_requirements();

    Ok(resolution.into_resolution_result(context))
}

pub async fn resolve_nodejs_variant_locator(context: &InstallContext<'_>, locator: &Locator, version: &zpm_semver::Version) -> Result<ResolutionResult, Error> {
    let (system, _, _)
        = PLATFORM_VARIANTS.iter()
            .find(|(_, file_name, _)| locator.ident.as_str() == &format!("@builtin/node-{}", file_name))
            .ok_or(Error::Unsupported)?;

    let mut resolution
        = Resolution::new_empty(locator.clone(), version.clone());

    resolution.requirements = system.to_requirements();

    Ok(resolution.into_resolution_result(context))
}

pub async fn fetch_nodejs_locator<'a>(context: &InstallContext<'a>, locator: &Locator, version: &zpm_semver::Version, is_mock_request: bool) -> Result<FetchResult, Error> {
    let (system, file_name, bin_file)
        = PLATFORM_VARIANTS.iter()
            .find(|(_, file_name, _)| locator.ident.as_str() == &format!("@builtin/node-{}", file_name))
            .ok_or(Error::Unsupported)?;

    if is_mock_request {
        let archive_path = context.package_cache.unwrap()
            .key_path(locator, ".zip");

        let package_directory = archive_path
            .with_join(&locator.ident.nm_subdir());

        return Ok(FetchResult::new_mock(archive_path, package_directory));
    }

    let project = context.project
        .expect("The project is required for fetching a nodejs package");

    let version_str
        = version.to_file_string();

    let url
        = format!("{}/v{}/node-v{}-{}.tar.gz", project.config.settings.node_dist_url.value, version_str, version_str, file_name);

    let package_cache = context.package_cache
        .expect("The package cache is required for fetching npm packages");

    let package_subdir
        = locator.ident.nm_subdir();

    let cached_blob = package_cache.ensure_blob(locator.clone(), ".zip", || async {
        let version_str
            = version.to_file_string();

        let project = context.project
            .expect("The project is required for fetching a nodejs package");

        let bytes
            = project.http_client.get(&url)?
                .send().await?
                .error_for_status()?
                .bytes().await?;

        let tar_data
            = zpm_formats::tar::unpack_tgz(&bytes)?;

        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct GeneratedManifest<'a> {
            name: &'a str,
            version: &'a str,
            os: &'a Os,
            cpu: &'a Cpu,
            prefer_unplugged: bool,
            bin: BinField,
        }

        let manifest = GeneratedManifest {
            name: locator.ident.as_str(),
            version: version_str.as_str(),
            os: system.os.as_ref().unwrap(),
            cpu: system.arch.as_ref().unwrap(),
            prefer_unplugged: true,
            bin: BinField::Map(BTreeMap::from([(Ident::from_str("node").unwrap(), RawPath {
                raw: bin_file.to_string(),
                path: Path::from_str(bin_file).unwrap(),
            })])),
        };

        let serialized_manifest
            = JsonDocument::to_string(&manifest)?;

        let entries
            = zpm_formats::tar::entries_from_tar(&tar_data)?
                .into_iter()
                .strip_first_segment()
                .filter(|entry| entry.name.as_str() == *bin_file)
                .chain(once(Entry::new_file(Path::from_str("package.json").unwrap(), Cow::Owned(serialized_manifest.into_bytes()))))
                .prepare_npm_entries(&locator.ident.nm_subdir())
                .collect_vec();

        Ok(package_cache.bundle_entries(entries)?)
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
