use serde::Deserialize;
use zpm_parsers::JsonDocument;
use zpm_primitives::{Locator, PypiRegistryReference};
use zpm_utils::ToFileString;

use crate::{
    error::Error,
    install::{FetchResult, InstallContext},
    pypi::{PypiDistribution, pypi_registry_base, encode_path_segment, select_best_wheel},
};

use super::PackageData;

#[derive(Clone, Debug, Deserialize)]
struct PypiVersionMetadata {
    #[serde(default)]
    urls: Vec<PypiDistribution>,
}

async fn resolve_artifact_url(context: &InstallContext<'_>, params: &PypiRegistryReference) -> Result<String, Error> {
    if let Some(url) = &params.url {
        return Ok(url.0.clone());
    }

    let project
        = context.project
        .expect("The project is required for fetching PyPI packages");

    let metadata_url
        = format!(
            "{}/pypi/{}/{}/json",
            pypi_registry_base(),
            encode_path_segment(params.ident.as_str()),
            encode_path_segment(&params.version.to_file_string()),
        );

    let bytes
        = project.http_client.cached_get(&metadata_url).await?;
    let metadata: PypiVersionMetadata
        = JsonDocument::hydrate_from_slice(&bytes[..])?;

    let wheel
        = select_best_wheel(&metadata.urls, None)
            .ok_or_else(|| Error::InvalidResolution(format!(
                "No wheel artifact found for {}@{}",
                params.ident.to_file_string(),
                params.version.to_file_string(),
            )))?;

    Ok(wheel.url.clone())
}

pub fn try_fetch_locator_sync(context: &InstallContext<'_>, locator: &Locator, _params: &PypiRegistryReference, is_mock_request: bool) -> Result<Option<FetchResult>, Error> {
    let package_cache
        = context.package_cache
        .expect("The package cache is required for fetching PyPI packages");

    if is_mock_request {
        let archive_path
            = package_cache
            .key_path(locator, ".zip");

        return Ok(Some(FetchResult::new_mock(archive_path.clone(), archive_path)));
    }

    let cache_entry
        = package_cache
        .check_cache_entry(locator.clone(), ".zip")?;

    Ok(cache_entry.map(|cache_entry| FetchResult::new(PackageData::Zip {
        archive_path: cache_entry.path.clone(),
        checksum: cache_entry.checksum,
        context_directory: cache_entry.path.clone(),
        package_directory: cache_entry.path,
    })))
}

pub async fn fetch_locator<'a>(context: &InstallContext<'a>, locator: &Locator, params: &PypiRegistryReference, is_mock_request: bool) -> Result<FetchResult, Error> {
    let package_cache
        = context.package_cache
        .expect("The package cache is required for fetching PyPI packages");

    if is_mock_request {
        let archive_path
            = package_cache
            .key_path(locator, ".zip");

        return Ok(FetchResult::new_mock(archive_path.clone(), archive_path));
    }

    let project
        = context.project
        .expect("The project is required for fetching PyPI packages");

    let artifact_url
        = resolve_artifact_url(context, params).await?;

    let cached_blob
        = package_cache.ensure_blob(locator.clone(), ".zip", || async {
            let response
                = project.http_client.get(&artifact_url)?
                    .send()
                    .await?;

            let bytes
                = response.bytes().await?;
            Ok(bytes.to_vec())
        }).await?.into_info();

    Ok(FetchResult::new(PackageData::Zip {
        archive_path: cached_blob.path.clone(),
        checksum: cached_blob.checksum,
        context_directory: cached_blob.path.clone(),
        package_directory: cached_blob.path,
    }))
}
