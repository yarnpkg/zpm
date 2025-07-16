use std::sync::Arc;

use crate::{error::Error, install::{FetchResult, InstallContext}, manifest::Manifest, primitives::{reference, Locator}, resolvers::Resolution};

use super::PackageData;

pub async fn fetch_locator<'a>(context: &InstallContext<'a>, locator: &Locator, params: &reference::UrlReference) -> Result<FetchResult, Error> {
    let project = context.project
        .expect("The project is required for fetching URL packages");

    let cached_blob = context.package_cache.unwrap().upsert_blob(locator.clone(), ".zip", || async {
        let response
            = project.http_client.get(&params.url)?.send().await?;

        let archive = response.bytes().await
            .map_err(|err| Error::RemoteRegistryError(Arc::new(err)))?;

        Ok(zpm_formats::convert::convert_tar_gz_to_zip(&locator.ident.nm_subdir(), archive)?)
    }).await?;

    let first_entry
        = zpm_formats::zip::first_entry_from_zip(&cached_blob.data)?;

    let manifest
        = sonic_rs::from_slice::<Manifest>(&first_entry.data)?;

    let resolution
        = Resolution::from_remote_manifest(locator.clone(), manifest.remote);

    let package_directory = cached_blob.info.path
        .with_join_str(locator.ident.nm_subdir());

    Ok(FetchResult {
        resolution: Some(resolution),
        package_data: PackageData::Zip {
            archive_path: cached_blob.info.path,
            checksum: cached_blob.info.checksum,
            context_directory: package_directory.clone(),
            package_directory,
        },
    })
}
