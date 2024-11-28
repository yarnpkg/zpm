use std::sync::Arc;

use crate::{error::Error, formats::{self, convert::convert_entries_to_zip, zip::ZipSupport}, install::{FetchResult, InstallContext, InstallOpResult}, manifest::Manifest, patch, primitives::{reference, Locator}, resolvers::Resolution};

use super::PackageData;

pub async fn fetch_locator<'a>(context: &InstallContext<'a>, locator: &Locator, params: &reference::PatchReference, dependencies: Vec<InstallOpResult>) -> Result<FetchResult, Error> {
    let project = context.project
        .expect("The project is required to fetch a patch package");

    let parent_data = dependencies[0].as_fetched();
    let original_data = dependencies[1].as_fetched();

    let cached_blob = context.package_cache.unwrap().upsert_blob(locator.clone(), ".zip", || async {
        let patch_path = match params.path.starts_with("~/") {
            true => project.project_cwd.with_join_str(&params.path[2..]),
            false => parent_data.package_data.context_directory().with_join_str(&params.path),
        };
            
        let patch_content = patch_path
            .fs_read_text_with_zip()?;

        let file_entries
            = original_data.package_data.file_entries()?;

        let patched_entries
            = patch::apply::apply_patch(file_entries, &patch_content)?;

        convert_entries_to_zip(&locator.ident, patched_entries)
    }).await?;

    let first_entry
        = formats::zip::first_entry_from_zip(&cached_blob.data);

    let manifest = first_entry
        .and_then(|entry|
            serde_json::from_slice::<Manifest>(&entry.data)
                .map_err(Arc::new)
                .map_err(Error::InvalidJsonData)
        )?;

    let resolution
        = Resolution::from_remote_manifest(locator.clone(), manifest.remote);

    let package_directory = cached_blob.path
        .with_join_str(locator.ident.nm_subdir());

    Ok(FetchResult {
        resolution: Some(resolution),
        package_data: PackageData::Zip {
            cached_blob,
            context_directory: package_directory.clone(),
            package_directory,
        },
    })
}
