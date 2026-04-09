use zpm_formats::iter_ext::IterExt;
use zpm_parsers::JsonDocument;
use zpm_primitives::{GitReference, Locator};

use crate::{
    error::{Error, remote_manifest_parse_error}, git, install::{FetchResult, InstallContext}, manifest::RemoteManifest, npm::NpmEntryExt, prepare, resolvers::Resolution
};

use super::PackageData;

pub async fn fetch_locator<'a>(context: &InstallContext<'a>, locator: &Locator, params: &GitReference, is_mock_request: bool) -> Result<FetchResult, Error> {
    let package_cache
        = context.package_cache
            .expect("The package cache is required for fetching git packages");

    let cache_packer
        = package_cache.packer();

    if is_mock_request {
        let archive_path = package_cache
            .key_path(locator, ".zip");

        let package_directory = archive_path
            .with_join(&locator.ident.nm_subdir());

        return Ok(FetchResult::new_mock(archive_path, package_directory));
    }

    let package_subdir
        = locator.ident.nm_subdir();
    let package_subdir_for_entries
        = package_subdir.clone();

    let pkg_blob = package_cache.upsert_blob(locator.clone(), ".zip", || async {
        let repository_path
            = git::clone_repository(context, &params.git.repo, &params.git.commit).await?;

        let pack_tgz = prepare::prepare_project(
            locator,
            &repository_path,
            &params.git.prepare_params,
        ).await?;

        let archive = tokio::task::spawn_blocking(move || -> Result<Vec<u8>, Error> {
            let pack_tar
                = zpm_formats::tar::unpack_tgz(&pack_tgz)?;

            let entries
                = zpm_formats::tar::entries_from_tar(&pack_tar)?
                    .into_iter()
                    .strip_first_segment()
                    .prepare_npm_entries(&package_subdir_for_entries)
                    .collect::<Vec<_>>();

            Ok(cache_packer.pack(entries)?)
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
            context_directory: package_directory.clone(),
            package_directory,
        },
    })
}
