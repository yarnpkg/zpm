use std::sync::Arc;

use crate::{error::Error, formats, git, install::{FetchResult, InstallContext}, manifest::RemoteManifest, prepare, primitives::{reference, Locator}, resolvers::Resolution};

use super::PackageData;

pub async fn fetch_locator<'a>(context: &InstallContext<'a>, locator: &Locator, reference: &reference::GitReference) -> Result<FetchResult, Error> {
    let (archive_path, data, checksum) = context.package_cache.unwrap().upsert_blob(locator.clone(), ".zip", || async {
        let repository_path
            = git::clone_repository(&reference.git.repo, &reference.git.commit).await?;

        let pack_tgz = prepare::prepare_project(
            &repository_path,
            &reference.git.prepare_params,
        ).await?;

        formats::convert::convert_tar_gz_to_zip(&locator.ident, pack_tgz.into())
    }).await?;

    let first_entry
        = formats::zip::first_entry_from_zip(&data);

    let remote_manifest = first_entry
        .and_then(|entry|
            serde_json::from_slice::<RemoteManifest>(&entry.data)
                .map_err(Arc::new)
                .map_err(Error::InvalidJsonData)
        )?;

    let resolution
        = Resolution::from_remote_manifest(locator.clone(), remote_manifest);

    let package_directory = archive_path
        .with_join_str(locator.ident.nm_subdir());

    Ok(FetchResult {
        resolution: Some(resolution),
        package_data: PackageData::Zip {
            archive_path,
            context_directory: package_directory.clone(),
            package_directory,
            data,
            checksum,
        },
    })
}
