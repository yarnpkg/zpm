use zpm_formats::iter_ext::IterExt;
use zpm_primitives::{GitReference, Locator};

use crate::{
    error::Error, git, install::{FetchResult, InstallContext}, manifest::RemoteManifest, npm::NpmEntryExt, prepare, resolvers::Resolution
};

use super::PackageData;

pub async fn fetch_locator<'a>(context: &InstallContext<'a>, locator: &Locator, params: &GitReference) -> Result<FetchResult, Error> {
    let package_cache = context.package_cache
        .expect("The package cache is required for fetching git packages");

    let pkg_blob = package_cache.upsert_blob(locator.clone(), ".zip", || async {
        let repository_path
            = git::clone_repository(context, &params.git.repo, &params.git.commit).await?;

        let pack_tgz = prepare::prepare_project(
            locator,
            &repository_path,
            &params.git.prepare_params,
        ).await?;

        let pack_tar
            = zpm_formats::tar::unpack_tgz(&pack_tgz)?;

        let entries
            = zpm_formats::tar::entries_from_tar(&pack_tar)?
                .into_iter()
                .strip_first_segment()
                .prepare_npm_entries(&locator.ident)
                .collect::<Vec<_>>();

        Ok(package_cache.bundle_entries(entries)?)
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
            context_directory: package_directory.clone(),
            package_directory,
        },
    })
}
