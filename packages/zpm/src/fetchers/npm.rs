use zpm_formats::iter_ext::IterExt;
use zpm_primitives::{Locator, RegistryReference};
use zpm_utils::Hash64;

use crate::{
    error::Error,
    http_npm::{self, AuthorizationMode, GetAuthorizationOptions},
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

    // Force the async path so `fetch_locator`'s refetch + tamper
    // detection runs under `--check-cache`.
    if context.check_checksums {
        return Ok(None);
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
        = http_npm::get_registry_for_ident(&project.config, Some(&params.ident), false)?;

    // When a custom archive URL is provided, use it directly; otherwise build from registry + path
    let (fetch_registry, fetch_path) = match &params.url {
        Some(url) => ("".to_string(), url.0.clone()),
        None => (registry_base.to_string(), npm::registry_url_for_package_data(&params.ident, &params.version)),
    };

    let package_cache = context.package_cache
        .expect("The package cache is required for fetching npm packages");
    let cache_packer
        = package_cache.packer();

    let package_subdir
        = params.ident.nm_subdir();
    let package_subdir_for_entries
        = package_subdir.clone();

    let authorization
        = http_npm::get_authorization(&GetAuthorizationOptions {
            configuration: &project.config,
            http_client: &project.http_client,
            registry: &registry_base,
            ident: Some(&params.ident),
            auth_mode: AuthorizationMode::RespectConfiguration,
            allow_oidc: false,
        }).await?;

    let fetch_archive = || async {
        let bytes
            = http_npm::get_uncached(&http_npm::NpmHttpParams {
                http_client: &project.http_client,
                registry: &fetch_registry,
                path: &fetch_path,
                authorization: authorization.as_deref(),
                otp: None,
            }).await?;

        let archive = tokio::task::spawn_blocking(move || -> Result<Vec<u8>, Error> {
            let tar_data
                = zpm_formats::tar::unpack_tgz(&bytes)?;

            let entries
                = zpm_formats::tar::entries_from_tar(&tar_data)?
                    .into_iter()
                    .strip_first_segment()
                    .prepare_npm_entries(&package_subdir_for_entries)?;

            Ok(cache_packer.pack(entries)?)
        }).await??;

        Ok(archive)
    };

    // Under --check-cache, hash the on-disk file before the refetch
    // overwrites it so we can compare against the fresh download.
    let pre_existing_hash = if context.check_checksums {
        package_cache
            .check_cache_entry(locator.clone(), ".zip")?
            .and_then(|entry| {
                entry.path.fs_read_prealloc().ok()
                    .map(|bytes| Hash64::from_data(&bytes))
            })
    } else {
        None
    };

    let cached_blob = if context.check_checksums {
        package_cache.refetch_blob(locator.clone(), ".zip", fetch_archive).await?
    } else {
        package_cache.ensure_blob(locator.clone(), ".zip", fetch_archive).await?
    }.into_info();

    if let (Some(pre_existing_hash), Some(fresh_hash)) = (pre_existing_hash, cached_blob.checksum.as_ref()) {
        if pre_existing_hash != *fresh_hash {
            return Err(Error::ChecksumMismatch(locator.clone()));
        }
    }

    let package_directory = cached_blob.path
        .with_join(&package_subdir);

    Ok(FetchResult::new(PackageData::Zip {
        archive_path: cached_blob.path,
        checksum: cached_blob.checksum,
        context_directory: package_directory.clone(),
        package_directory,
    }))
}
