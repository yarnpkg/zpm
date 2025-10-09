use zpm_formats::iter_ext::IterExt;
use zpm_parsers::JsonDocument;
use zpm_primitives::{Locator, TarballReference};

use crate::{
    error::Error, install::{FetchResult, InstallContext, InstallOpResult}, manifest::Manifest, npm::NpmEntryExt, resolvers::Resolution
};

use super::PackageData;

pub async fn fetch_locator<'a>(context: &InstallContext<'a>, locator: &Locator, params: &TarballReference, dependencies: Vec<InstallOpResult>) -> Result<FetchResult, Error> {
    let parent_data
        = dependencies[0].as_fetched();

    let tarball_path = parent_data.package_data
        .context_directory()
        .with_join_str(&params.path);

    let package_cache = context.package_cache
        .expect("The package cache is required for fetching tarball packages");

    let package_subdir
        = locator.ident.nm_subdir();

    let cached_blob = package_cache.upsert_blob(locator.clone(), ".zip", || async {
        let tgz_data
            = tarball_path.fs_read()?;
        let tar_data
            = zpm_formats::tar::unpack_tgz(&tgz_data)?;

        let entries
            = zpm_formats::tar::entries_from_tar(&tar_data)?
                .into_iter()
                .strip_first_segment()
                .prepare_npm_entries(&package_subdir)
                .collect();

        Ok(package_cache.bundle_entries(entries)?)
    }).await?;

    let first_entry
        = zpm_formats::zip::first_entry_from_zip(&cached_blob.data)?;

    let manifest: Manifest
        = JsonDocument::hydrate_from_slice(&first_entry.data)?;

    let resolution
        = Resolution::from_remote_manifest(locator.clone(), manifest.remote);

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
