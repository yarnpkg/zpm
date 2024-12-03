use crate::{error::Error, formats, git, install::{FetchResult, InstallContext}, manifest::RemoteManifest, prepare, primitives::{reference, Locator}, resolvers::Resolution};

use super::PackageData;

pub async fn fetch_locator<'a>(context: &InstallContext<'a>, locator: &Locator, params: &reference::GitReference) -> Result<FetchResult, Error> {
    let pkg_blob = context.package_cache.unwrap().upsert_blob(locator.clone(), ".zip", || async {
        let repository_path
            = git::clone_repository(&params.git.repo, &params.git.commit).await?;

        let pack_tgz = prepare::prepare_project(
            locator,
            &repository_path,
            &params.git.prepare_params,
        ).await?;

        formats::convert::convert_tar_gz_to_zip(&locator.ident, pack_tgz.into())
    }).await?;

    let first_entry
        = formats::zip::first_entry_from_zip(&pkg_blob.data);

    let remote_manifest = first_entry
        .and_then(|entry| Ok(sonic_rs::from_slice::<RemoteManifest>(&entry.data)?))?;

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
