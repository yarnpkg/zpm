use std::sync::Arc;

use zpm_config::ConfigExt;
use zpm_formats::iter_ext::IterExt;
use zpm_primitives::{Locator, RegistryReference};

use crate::{
    error::Error,
    http_npm,
    install::{FetchResult, InstallContext},
    npm::{self, NpmEntryExt},
};

use super::PackageData;

fn get_mock_fetch_result(context: &InstallContext, locator: &Locator, params: &RegistryReference) -> Result<FetchResult, Error> {
    let archive_path = context.package_cache.unwrap()
        .key_path(locator, ".zip");

    let package_directory = archive_path
        .with_join_str(params.ident.nm_subdir());

    Ok(FetchResult::new(PackageData::MissingZip {
        archive_path,
        context_directory: package_directory.clone(),
        package_directory,
    }))
}

pub fn try_fetch_locator_sync(context: &InstallContext, locator: &Locator, params: &RegistryReference, is_mock_request: bool) -> Result<Option<FetchResult>, Error> {
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

pub async fn fetch_locator<'a>(context: &InstallContext<'a>, locator: &Locator, params: &RegistryReference, is_mock_request: bool) -> Result<FetchResult, Error> {
    let project = context.project
        .expect("The project is required for resolving a workspace package");

    if is_mock_request {
        return Ok(get_mock_fetch_result(context, locator, params)?);
    }

    let registry_base
        = project.config.registry_base_for(&params.ident);
    let registry_path
        = npm::registry_url_for_package_data(&params.ident, &params.version);

    let package_cache = context.package_cache
        .expect("The package cache is required for fetching npm packages");

    let cached_blob = package_cache.ensure_blob(locator.clone(), ".zip", || async {
        let response
            = http_npm::get(&http_npm::NpmHttpParams {
                http_client: &project.http_client,
                registry: &registry_base,
                path: &registry_path,
                authorization: None,
            }).await?;
        let tgz_data = response.bytes().await
            .map_err(|err| Error::RemoteRegistryError(Arc::new(err)))?;

        let tar_data
            = zpm_formats::tar::unpack_tgz(&tgz_data)?;

        let entries
            = zpm_formats::tar::entries_from_tar(&tar_data)?
                .into_iter()
                .strip_first_segment()
                .prepare_npm_entries(&params.ident)
                .collect::<Vec<_>>();

        Ok(package_cache.bundle_entries(entries)?)
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
