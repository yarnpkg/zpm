use zpm_formats::iter_ext::IterExt;
use zpm_parsers::JsonDocument;
use zpm_primitives::{Locator, TarballReference};
use zpm_utils::{FromFileString, Path};

use crate::{
    error::Error, install::{FetchResult, InstallContext, InstallOpResult}, manifest::RemoteManifest, npm::NpmEntryExt, resolvers::Resolution
};

use super::PackageData;

pub async fn fetch_locator<'a>(context: &InstallContext<'a>, locator: &Locator, params: &TarballReference, is_mock_request: bool, dependencies: Vec<InstallOpResult>) -> Result<FetchResult, Error> {
    let package_cache
        = context.package_cache
            .expect("The package cache is required for fetching tarball packages");

    let cache_packer
        = package_cache.packer();

    if is_mock_request {
        let archive_path = package_cache
            .key_path(locator, ".zip");

        let package_directory = archive_path
            .with_join(&locator.ident.nm_subdir());

        return Ok(FetchResult::new_mock(archive_path, package_directory));
    }

    let tarball_relative_path
        = Path::from_file_string(&params.path)?;

    let tarball_path = if tarball_relative_path.is_absolute() {
        tarball_relative_path
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

    let cached_blob = package_cache.refetch_blob_data(locator.clone(), ".zip", || async {
        let tgz_data
            = tarball_path.fs_read()?;

        let archive = tokio::task::spawn_blocking(move || -> Result<Vec<u8>, Error> {
            let tar_data
                = zpm_formats::tar::unpack_tgz(&tgz_data)?;

            let entries
                = zpm_formats::tar::entries_from_tar(&tar_data)?
                    .into_iter()
                    .strip_first_segment()
                    .prepare_npm_entries(&package_subdir_for_entries)?;

            Ok(cache_packer.pack(entries)?)
        }).await??;

        Ok(archive)
    }).await?;

    let first_entry
        = zpm_formats::zip::first_entry_from_zip(&cached_blob.data)?;

    let manifest: RemoteManifest
        = JsonDocument::hydrate_from_slice(&first_entry.data)?;

    let resolution
        = Resolution::from_remote_manifest(locator.clone(), manifest);

    let package_directory = cached_blob.info.path
        .with_join(&package_subdir);

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
