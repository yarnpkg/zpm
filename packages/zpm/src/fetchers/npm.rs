use std::collections::HashSet;
use std::sync::{Arc, LazyLock, Mutex};

use tokio::time::{sleep, Duration};
use zpm_config::ConfigExt;
use zpm_formats::iter_ext::IterExt;
use zpm_primitives::{Locator, RegistryReference};

use crate::{
    error::Error,
    install::{FetchResult, InstallContext},
    npm::{self, NpmEntryExt},
    report::current_report,
};

use super::PackageData;

static WARNED_REGISTRIES: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

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

    let registry_base = project.config.registry_base_for(&params.ident);
    let registry_url = npm::registry_url_for_package_data(&registry_base, &params.ident, &params.version);

    let package_cache = context.package_cache
        .expect("The package cache is required for fetching npm packages");

    let cached_blob = package_cache.ensure_blob(locator.clone(), ".zip", || async {
        let fetch_future = async {
            let response = project.http_client.get(&registry_url)?.send().await?;
            let tgz_data = response.bytes().await
                .map_err(|err| Error::RemoteRegistryError(Arc::new(err)))?;
            Ok::<_, Error>(tgz_data)
        };

        let warning_future = async {
            sleep(Duration::from_secs(15)).await;

            // Check if we should warn about this registry
            let should_warn = {
                let mut warned = WARNED_REGISTRIES.lock().unwrap();
                if !warned.contains(&registry_base) {
                    warned.insert(registry_base.clone());
                    true
                } else {
                    false
                }
            }; // Lock is dropped here

            if should_warn {
                current_report().await.as_mut().map(|report| {
                    report.warn(format!("Requests to {} are taking suspiciously long...", registry_base));
                });
            }
        };

        let tgz_data = tokio::select! {
            result = fetch_future => result?,
            _ = warning_future => {
                // Warning was shown, now wait for the request to complete
                let response = project.http_client.get(&registry_url)?.send().await?;
                response.bytes().await
                    .map_err(|err| Error::RemoteRegistryError(Arc::new(err)))?
            }
        };

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
