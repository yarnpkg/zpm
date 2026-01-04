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

pub fn try_fetch_locator_sync(context: &InstallContext, locator: &Locator, params: &RegistryReference, is_mock_request: bool) -> Result<Option<FetchResult>, Error> {
    if is_mock_request {
        let archive_path = context.package_cache.unwrap()
            .key_path(locator, ".zip");

        let package_directory = archive_path
            .with_join(&params.ident.nm_subdir());

        return Ok(Some(FetchResult::new_mock(archive_path, package_directory)));
    }

    let cache_entry = context.package_cache.unwrap()
        .check_cache_entry(locator.clone(), ".zip")?;

    Ok(cache_entry.map(|cache_entry| {
        let package_directory = cache_entry.path
            .with_join(&params.ident.nm_subdir());

        FetchResult::new(PackageData::Zip {
            archive_path: cache_entry.path,
            checksum: cache_entry.checksum,
            context_directory: package_directory.clone(),
            package_directory,
        })
    }))
}

pub async fn fetch_locator<'a>(context: &InstallContext<'a>, locator: &Locator, params: &RegistryReference, is_mock_request: bool) -> Result<FetchResult, Error> {
    if is_mock_request {
        let archive_path = context.package_cache.unwrap()
            .key_path(locator, ".zip");

        let package_directory = archive_path
            .with_join(&params.ident.nm_subdir());

        return Ok(FetchResult::new_mock(archive_path, package_directory));
    }

    let project = context.project
        .expect("The project is required for resolving a workspace package");

    let registry_base
        = project.config.registry_base_for(&params.ident);
    let registry_path
        = npm::registry_url_for_package_data(&params.ident, &params.version);

    let package_cache = context.package_cache
        .expect("The package cache is required for fetching npm packages");

    let package_subdir
        = params.ident.nm_subdir();

    let cached_blob = package_cache.ensure_blob(locator.clone(), ".zip", || async {
        let bytes
            = http_npm::get_raw(&http_npm::NpmHttpParams {
                http_client: &project.http_client,
                registry: &registry_base,
                path: &registry_path,
                authorization: None,
                otp: None,
            }).await?;

        let tar_data
            = zpm_formats::tar::unpack_tgz(&bytes)?;

        let entries
            = zpm_formats::tar::entries_from_tar(&tar_data)?
                .into_iter()
                .strip_first_segment()
                .prepare_npm_entries(&package_subdir)
                .collect::<Vec<_>>();

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
