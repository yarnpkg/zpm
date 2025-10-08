use std::sync::Arc;

use zpm_formats::iter_ext::IterExt;
use zpm_parsers::JsonDocument;
use zpm_primitives::{Locator, UrlReference};

use crate::{
    error::Error, install::{FetchResult, InstallContext}, manifest::Manifest, npm::NpmEntryExt, resolvers::Resolution
};

use super::PackageData;

pub async fn fetch_locator<'a>(context: &InstallContext<'a>, locator: &Locator, params: &UrlReference) -> Result<FetchResult, Error> {
    let project = context.project
        .expect("The project is required for fetching URL packages");

    let package_cache = context.package_cache
        .expect("The package cache is required for fetching URL packages");

    let cached_blob = package_cache.upsert_blob(locator.clone(), ".zip", || async {
        let response
            = project.http_client.get(&params.url)?.send().await?;

        let tgz_data = response.bytes().await
            .map_err(|err| Error::RemoteRegistryError(Arc::new(err)))?;
        let tar_data
            = zpm_formats::tar::unpack_tgz(&tgz_data)?;

        let entries
            = zpm_formats::tar::entries_from_tar(&tar_data)?
                .into_iter()
                .strip_first_segment()
                .prepare_npm_entries(&locator.ident)
                .collect::<Vec<_>>();

        Ok(package_cache.bundle_entries(entries)?)
    }).await?;

    let first_entry
        = zpm_formats::zip::first_entry_from_zip(&cached_blob.data)?;

    let manifest: Manifest
        = JsonDocument::hydrate_from_slice(&first_entry.data)?;

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
