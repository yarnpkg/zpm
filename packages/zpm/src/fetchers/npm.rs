use std::sync::Arc;

use crate::{error::Error, http::http_client, install::{FetchResult, InstallContext}, primitives::{reference, Locator}};

use super::PackageData;

fn get_mock_fetch_result(context: &InstallContext, locator: &Locator, params: &reference::RegistryReference) -> Result<FetchResult, Error> {
    let archive_path = context.package_cache.unwrap()
        .key_path(locator, ".zip")?;

    let package_directory = archive_path
        .with_join_str(params.ident.nm_subdir());

    Ok(FetchResult::new(PackageData::MissingZip {
        archive_path,
        context_directory: package_directory.clone(),
        package_directory,
    }))
}

pub fn try_fetch_locator_sync(context: &InstallContext, locator: &Locator, params: &reference::RegistryReference, is_mock_request: bool) -> Result<Option<FetchResult>, Error> {
    if is_mock_request {
        return Ok(Some(get_mock_fetch_result(context, locator, params)?));
    }

    let cache_entry = context.package_cache.unwrap()
        .check_cache_entry(locator.clone(), ".zip")?;

    Ok(cache_entry.map(|cache_entry| {
        let package_directory = cache_entry.path
            .with_join_str(params.ident.nm_subdir());

        FetchResult::new(PackageData::Zip {
            archive_path: cache_entry.path,
            checksum: cache_entry.checksum,
            context_directory: package_directory.clone(),
            package_directory,
        })
    }))
}

pub async fn fetch_locator<'a>(context: &InstallContext<'a>, locator: &Locator, params: &reference::RegistryReference, is_mock_request: bool) -> Result<FetchResult, Error> {
    let project = context.project
        .expect("The project is required for resolving a workspace package");

    if is_mock_request {
        return Ok(get_mock_fetch_result(context, locator, params)?);
    }

    let registry_url
        = project.config.registry_url_for_package_data(params);

    let cached_blob = context.package_cache.unwrap().ensure_blob(locator.clone(), ".zip", || async {
        let client = http_client()?;

        let response = client.get(registry_url.clone()).send().await
            .map_err(|err| Error::RemoteRegistryError(Arc::new(err)))?;

        let archive = response.bytes().await
            .map_err(|err| Error::RemoteRegistryError(Arc::new(err)))?;

        Ok(zpm_formats::convert::convert_tar_gz_to_zip(&params.ident.nm_subdir(), archive)?)
    }).await?.into_info();

    let package_directory = cached_blob.path
        .with_join_str(params.ident.nm_subdir());

    Ok(FetchResult::new(PackageData::Zip {
        archive_path: cached_blob.path,
        checksum: cached_blob.checksum,
        context_directory: package_directory.clone(),
        package_directory,
    }))
}
