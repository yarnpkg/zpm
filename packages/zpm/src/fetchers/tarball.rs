use bytes::Bytes;

use crate::{error::Error, install::{FetchResult, InstallContext, InstallOpResult}, manifest::Manifest, primitives::{reference, Locator}, resolvers::Resolution};

use super::PackageData;

pub async fn fetch_locator<'a>(context: &InstallContext<'a>, locator: &Locator, params: &reference::TarballReference, dependencies: Vec<InstallOpResult>) -> Result<FetchResult, Error> {
    let parent_data = dependencies[0].as_fetched();

    let tarball_path = parent_data.package_data
        .context_directory()
        .with_join_str(&params.path);

    let cached_blob = context.package_cache.unwrap().upsert_blob(locator.clone(), ".zip", || async {
        Ok(zpm_formats::convert::convert_tar_gz_to_zip_async(&locator.ident.nm_subdir(), Bytes::from(tarball_path.fs_read()?)).await?)
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
