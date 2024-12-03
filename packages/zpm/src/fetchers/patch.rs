use crate::{error::Error, formats::{self, convert::convert_entries_to_zip, zip::ZipSupport}, install::{FetchResult, InstallContext, InstallOpResult}, manifest::Manifest, patch, primitives::{reference, Locator}, resolvers::Resolution};

use super::PackageData;

pub async fn fetch_locator<'a>(context: &InstallContext<'a>, locator: &Locator, params: &reference::PatchReference, dependencies: Vec<InstallOpResult>) -> Result<FetchResult, Error> {
    let project = context.project
        .expect("The project is required to fetch a patch package");

    let parent_data = dependencies[0].as_fetched();
    let original_data = dependencies[1].as_fetched();

    let cached_blob = context.package_cache.unwrap().upsert_blob(locator.clone(), ".zip", || async {
        let original_bytes = match &original_data.package_data {
            PackageData::Zip {archive_path, ..} => Some(archive_path.fs_read()?),
            _ => None,
        };

        let original_entries = match &original_data.package_data {
            PackageData::Local {package_directory, ..} => {
                formats::entries_from_folder(&package_directory)?
            },

            PackageData::Zip {..} => {
                let entries
                    = formats::zip::entries_from_zip(original_bytes.as_ref().unwrap())?;

                let package_subpath
                    = original_data.package_data.package_subpath();

                formats::strip_prefix(entries, package_subpath.as_str())
            },

            PackageData::MissingZip {..} => {
                return Err(Error::Unsupported);
            },
        };

        let patch_path = match params.path.starts_with("~/") {
            true => project.project_cwd.with_join_str(&params.path[2..]),
            false => parent_data.package_data.context_directory().with_join_str(&params.path),
        };
            
        let patch_content = patch_path
            .fs_read_text_with_zip()?;

        let patched_entries
            = patch::apply::apply_patch(original_entries, &patch_content)?;

        convert_entries_to_zip(&locator.ident, patched_entries)
    }).await?;

    let first_entry
        = formats::zip::first_entry_from_zip(&cached_blob.data);

    let manifest = first_entry
        .and_then(|entry| Ok(sonic_rs::from_slice::<Manifest>(&entry.data)?))?;

    let resolution
        = Resolution::from_remote_manifest(locator.clone(), manifest.remote);

    let package_directory = cached_blob.info.path
        .with_join_str(locator.ident.nm_subdir());

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
