use crate::{error::Error, install::{FetchResult, InstallContext, InstallOpResult}, manifest::RemoteManifest, primitives::{reference, Locator}, resolvers::Resolution};

use super::PackageData;

pub async fn fetch_locator<'a>(context: &InstallContext<'a>, locator: &Locator, params: &reference::FolderReference, dependencies: Vec<InstallOpResult>) -> Result<FetchResult, Error> {
    let parent_data = dependencies[0].as_fetched();

    let context_directory = parent_data.package_data
        .context_directory()
        .with_join_str(&params.path);

    let pkg_blob = context.package_cache.unwrap().upsert_blob(locator.clone(), ".zip", || async {
        Ok(zpm_formats::convert::convert_folder_to_zip(&locator.ident.nm_subdir(), &context_directory)?)
    }).await?;

    let first_entry
        = zpm_formats::zip::first_entry_from_zip(&pkg_blob.data)?;

    let remote_manifest
        = sonic_rs::from_slice::<RemoteManifest>(&first_entry.data)?;

    let resolution
        = Resolution::from_remote_manifest(locator.clone(), remote_manifest);

    let package_directory = pkg_blob.info.path
        .with_join_str(locator.ident.nm_subdir());

    Ok(FetchResult {
        resolution: Some(resolution),
        package_data: PackageData::Zip {
            archive_path: pkg_blob.info.path,
            checksum: pkg_blob.info.checksum,
            context_directory,
            package_directory,
        },
    })
}
