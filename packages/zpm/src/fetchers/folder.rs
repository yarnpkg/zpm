use zpm_parsers::JsonDocument;
use zpm_primitives::{FolderReference, Locator};
use zpm_utils::{FromFileString, Path};

use crate::{
    error::{Error, remote_manifest_parse_error}, install::{FetchResult, InstallContext, InstallOpResult}, manifest::RemoteManifest, npm::NpmEntryExt, resolvers::Resolution
};

use super::PackageData;

pub async fn fetch_locator<'a>(context: &InstallContext<'a>, locator: &Locator, params: &FolderReference, is_mock_request: bool, dependencies: Vec<InstallOpResult>) -> Result<FetchResult, Error> {
    let package_cache
        = context.package_cache
            .expect("The package cache is required for fetching folder packages");

    let cache_packer
        = package_cache.packer();

    if is_mock_request {
        let archive_path = package_cache
            .key_path(locator, ".zip");

        let package_directory = archive_path
            .with_join(&locator.ident.nm_subdir());

        return Ok(FetchResult::new_mock(archive_path, package_directory));
    }

    let folder_relative_path
        = Path::from_file_string(&params.path)?;

    let context_directory = if folder_relative_path.is_absolute() {
        folder_relative_path
    } else {
        let parent_data
            = dependencies.first()
                .ok_or(Error::Unsupported)?
                .as_fetched();

        parent_data.package_data
            .context_directory()
            .with_join_str(&params.path)
    };

    let package_subdir
        = locator.ident.nm_subdir();

    let package_subdir_for_entries
        = package_subdir.clone();
    let context_directory_for_entries
        = context_directory.clone();

    let pkg_blob = package_cache.upsert_blob(locator.clone(), ".zip", || async {
        let archive = tokio::task::spawn_blocking(move || -> Result<Vec<u8>, Error> {
            let entries
                = zpm_formats::entries_from_folder(&context_directory_for_entries)?
                    .into_iter()
                    .prepare_npm_entries(&package_subdir_for_entries)
                    .collect::<Vec<_>>();

            cache_packer.pack(entries)
        }).await??;

        Ok(archive)
    }).await?;

    let first_entry
        = zpm_formats::zip::first_entry_from_zip(&pkg_blob.data)?;

    let remote_manifest: RemoteManifest
        = JsonDocument::hydrate_from_slice(&first_entry.data)
            .map_err(|error| remote_manifest_parse_error(locator, "package archive", "package.json", error))?;

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
