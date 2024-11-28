use std::sync::Arc;

use crate::{error::Error, formats, install::{FetchResult, InstallContext, InstallOpResult}, manifest::RemoteManifest, primitives::{reference, Locator}, resolvers::Resolution};

use super::PackageData;

pub async fn fetch_locator<'a>(context: &InstallContext<'a>, locator: &Locator, params: &reference::FolderReference, dependencies: Vec<InstallOpResult>) -> Result<FetchResult, Error> {
    let parent_data = dependencies[0].as_fetched();

    let context_directory = parent_data.package_data
        .context_directory()
        .with_join_str(&params.path);

    let cached_blob = context.package_cache.unwrap().upsert_blob(locator.clone(), ".zip", || async {
        formats::convert::convert_folder_to_zip(&locator.ident, &context_directory)
    }).await?;

    let first_entry
        = formats::zip::first_entry_from_zip(&cached_blob.data);

    let remote_manifest = first_entry
        .and_then(|entry|
            serde_json::from_slice::<RemoteManifest>(&entry.data)
                .map_err(Arc::new)
                .map_err(Error::InvalidJsonData)
        )?;

    let resolution
        = Resolution::from_remote_manifest(locator.clone(), remote_manifest);

    let package_directory = cached_blob.path
        .with_join_str(locator.ident.nm_subdir());

    Ok(FetchResult {
        resolution: Some(resolution),
        package_data: PackageData::Zip {
            cached_blob,
            context_directory,
            package_directory,
        },
    })
}
