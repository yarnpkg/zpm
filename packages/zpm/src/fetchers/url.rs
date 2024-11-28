use std::sync::Arc;

use crate::{error::Error, formats, http::http_client, install::{FetchResult, InstallContext}, manifest::Manifest, primitives::{reference, Locator}, resolvers::Resolution};

use super::PackageData;

pub async fn fetch_locator<'a>(context: &InstallContext<'a>, locator: &Locator, params: &reference::UrlReference) -> Result<FetchResult, Error> {
    let cached_blob = context.package_cache.unwrap().upsert_blob(locator.clone(), ".zip", || async {
        let client = http_client()?;

        let response = client.get(&params.url).send().await
            .map_err(|err| Error::RemoteRegistryError(Arc::new(err)))?;

        let archive = response.bytes().await
            .map_err(|err| Error::RemoteRegistryError(Arc::new(err)))?;

        formats::convert::convert_tar_gz_to_zip(&locator.ident, archive)
    }).await?;

    let first_entry = formats::zip::first_entry_from_zip(&cached_blob.data);
    let manifest = first_entry
        .and_then(|entry|
            serde_json::from_slice::<Manifest>(&entry.data)
                .map_err(Arc::new)
                .map_err(Error::InvalidJsonData)
        )?;

    let resolution
        = Resolution::from_remote_manifest(locator.clone(), manifest.remote);

    let package_directory = cached_blob.path
        .with_join_str(locator.ident.nm_subdir());

    Ok(FetchResult {
        resolution: Some(resolution),
        package_data: PackageData::Zip {
            cached_blob,
            context_directory: package_directory.clone(),
            package_directory,
        },
    })
}
