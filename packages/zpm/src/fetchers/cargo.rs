use sha2::Digest;
use zpm_primitives::{CargoGitReference, CargoPathReference, CargoRegistryReference, Locator};

use crate::{
    cargo::{CargoContext, git, index},
    error::Error,
    install::{FetchResult, InstallContext},
};

use super::PackageData;

/// Crates are cached as published; the cargo linker unpacks them.
const CRATE_EXTENSION: &str
    = ".crate";

pub fn try_fetch_registry_sync(context: &InstallContext<'_>, locator: &Locator, is_mock_request: bool) -> Result<Option<FetchResult>, Error> {
    let package_cache
        = context.package_cache
            .expect("The package cache is required for fetching crates");

    if is_mock_request {
        let archive_path
            = package_cache.key_path(locator, CRATE_EXTENSION);

        return Ok(Some(FetchResult::new_mock(archive_path.clone(), archive_path)));
    }

    let cache_entry
        = package_cache.check_cache_entry(locator.clone(), CRATE_EXTENSION)?;

    Ok(cache_entry.map(|cache_entry| FetchResult::new(PackageData::Zip {
        archive_path: cache_entry.path.clone(),
        checksum: cache_entry.checksum,
        context_directory: cache_entry.path.clone(),
        package_directory: cache_entry.path,
    })))
}

pub async fn fetch_registry(context: &InstallContext<'_>, locator: &Locator, params: &CargoRegistryReference, is_mock_request: bool) -> Result<FetchResult, Error> {
    let package_cache
        = context.package_cache
            .expect("The package cache is required for fetching crates");

    let project
        = context.project
            .expect("The project is required for fetching crates");

    if is_mock_request {
        let archive_path
            = package_cache.key_path(locator, CRATE_EXTENSION);

        return Ok(FetchResult::new_mock(archive_path.clone(), archive_path));
    }

    let cached_blob = package_cache.ensure_blob(locator.clone(), CRATE_EXTENSION, || async {
        let cargo_context
            = CargoContext::new(project);

        let name
            = locator.ident.as_str();

        // The index is the only place the checksums are published at; the
        // listing is read from the disk when it already has the version
        let summary
            = index::fetch_version(&cargo_context, &params.registry, name, &params.version.to_semver()).await?;

        let expected_checksum
            = summary.checksum.clone().unwrap_or_default();

        let config
            = index::fetch_config(&cargo_context, &params.registry).await?;

        let download_url
            = config.download_url(name, params.version.as_str(), &expected_checksum);

        let (_, bytes) = project.http_client.get(&download_url)?
            .header("authorization", index::download_authorization(&cargo_context, &params.registry, &config, &download_url))
            .send_bytes()
            .await?;

        if hex::encode(sha2::Sha256::digest(&bytes)) != expected_checksum.to_ascii_lowercase() {
            return Err(Error::ChecksumMismatch(locator.clone()));
        }

        Ok(bytes.to_vec())
    }).await?.into_info();

    Ok(FetchResult::new(PackageData::Zip {
        archive_path: cached_blob.path.clone(),
        checksum: cached_blob.checksum,
        context_directory: cached_blob.path.clone(),
        package_directory: cached_blob.path,
    }))
}

/// Git crates are used from their checkout, which lives in the global
/// folder; the commit is what guarantees their content.
pub async fn fetch_git(context: &InstallContext<'_>, locator: &Locator, params: &CargoGitReference) -> Result<FetchResult, Error> {
    let project
        = context.project
            .expect("The project is required for fetching crates");

    let cargo_context
        = CargoContext::new(project);

    let checkout_dir
        = git::ensure_checkout(&cargo_context, &params.source, &params.commit).await?;

    let packages
        = git::read_packages(&checkout_dir, &params.source, &params.commit, &cargo_context.registries)?;

    let package = packages.get(locator.ident.as_str())
        .ok_or_else(|| Error::CargoError(format!("No crate named {} was found in {}", locator.ident.as_str(), params.source.url())))?;

    Ok(FetchResult::new(PackageData::Local {
        package_directory: package.manifest.dir.clone(),
        is_synthetic_package: true,
    }))
}

pub fn fetch_path(context: &InstallContext<'_>, _locator: &Locator, params: &CargoPathReference) -> Result<FetchResult, Error> {
    let project
        = context.project
            .expect("The project is required for fetching crates");

    Ok(FetchResult::new(PackageData::Local {
        package_directory: project.project_cwd.with_join_str(&params.path),
        is_synthetic_package: true,
    }))
}
