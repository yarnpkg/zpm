use zpm_parsers::JsonDocument;
use zpm_primitives::{FolderReference, Locator};

use crate::{
    error::Error, install::{FetchResult, InstallContext, InstallOpResult}, manifest::RemoteManifest, npm::NpmEntryExt, resolvers::Resolution
};

use super::PackageData;

pub async fn fetch_locator<'a>(context: &InstallContext<'a>, locator: &Locator, params: &FolderReference, dependencies: Vec<InstallOpResult>) -> Result<FetchResult, Error> {
    let parent_data
        = dependencies[0].as_fetched();

    let context_directory = parent_data.package_data
        .context_directory()
        .with_join_str(&params.path);

    let package_cache = context.package_cache
        .expect("The package cache is required for fetching folder packages");

    let package_subdir
        = locator.ident.nm_subdir();

    let pkg_blob = package_cache.upsert_blob(locator.clone(), ".zip", || async {
        let entries
            = zpm_formats::entries_from_folder(&context_directory)?
                .into_iter()
                .prepare_npm_entries(&package_subdir)
                .collect::<Vec<_>>();

        Ok(package_cache.bundle_entries(entries)?)
    }).await?;

    let first_entry
        = zpm_formats::zip::first_entry_from_zip(&pkg_blob.data)?;

    let remote_manifest: RemoteManifest
        = JsonDocument::hydrate_from_slice(&first_entry.data)?;

    let resolution
        = Resolution::from_remote_manifest(locator.clone(), remote_manifest);

    let package_directory = pkg_blob.info.path
        .with_join(&package_subdir);

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
