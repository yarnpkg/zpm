use std::sync::Arc;

use crate::{error::Error, formats, http::http_client, install::{FetchResult, InstallContext}, primitives::{reference, Locator}};

use super::PackageData;

pub async fn fetch_locator<'a>(context: &InstallContext<'a>, locator: &Locator, params: &reference::RegistryReference, is_mock_request: bool) -> Result<FetchResult, Error> {
    let project = context.project
        .expect("The project is required for resolving a workspace package");

    if is_mock_request {
        let archive_path = context.package_cache.unwrap()
            .key_path(locator, ".zip")?;

        let package_directory = archive_path
            .with_join_str(params.ident.nm_subdir());

        return Ok(FetchResult::new(PackageData::MissingZip {
            archive_path,
            context_directory: package_directory.clone(),
            package_directory,
        }));
    }

    let registry_url
        = project.config.registry_url_for_package_data(params);

    let (archive_path, data, checksum) = context.package_cache.unwrap().upsert_blob_or_mock(is_mock_request, locator.clone(), ".zip", || async {
        let client = http_client()?;

        let response = client.get(registry_url.clone()).send().await
            .map_err(|err| Error::RemoteRegistryError(Arc::new(err)))?;

        let archive = response.bytes().await
            .map_err(|err| Error::RemoteRegistryError(Arc::new(err)))?;

        formats::convert::convert_tar_gz_to_zip(&params.ident, archive)
    }).await?;

    let package_directory = archive_path
        .with_join_str(params.ident.nm_subdir());

    Ok(FetchResult::new(PackageData::Zip {
        archive_path,
        context_directory: package_directory.clone(),
        package_directory,
        data,
        checksum,
    }))
}
