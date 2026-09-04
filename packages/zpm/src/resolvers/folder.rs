use zpm_primitives::{Descriptor, FolderRange, FolderReference, Locator};
use zpm_utils::{FromFileString, Hash64Writer, Path};

use crate::{
    error::Error,
    fetchers,
    install::{InstallContext, InstallOpResult, IntoResolutionResult, ResolutionResult},
    npm::NpmEntryExt,
};

pub async fn resolve_descriptor(context: &InstallContext<'_>, descriptor: &Descriptor, range: &FolderRange, dependencies: Vec<InstallOpResult>) -> Result<ResolutionResult, Error> {
    let hash
        = compute_folder_hash(context, descriptor, range, &dependencies).await?;

    let locator = descriptor.resolve_with(FolderReference {
        path: range.path.to_string(),
        hash: Some(hash),
    }.into());

    let fetch_result
        = fetchers::fetch_locator(context.clone(), &locator, false, dependencies).await?;

    fetch_result.into_resolution_result(context)
}

async fn compute_folder_hash(context: &InstallContext<'_>, descriptor: &Descriptor, range: &FolderRange, dependencies: &[InstallOpResult]) -> Result<zpm_utils::Hash64, Error> {
    let package_cache
        = context.package_cache
            .expect("The package cache is required for resolving folder packages");

    let folder_relative_path
        = Path::from_file_string(&range.path)?;

    let context_directory = if folder_relative_path.is_absolute() {
        context.absolute_source_path(&folder_relative_path)?
    } else {
        let parent_data
            = dependencies.first()
                .ok_or(Error::Unsupported)?
                .as_fetched();

        context.relative_source_path(parent_data.package_data.context_directory(), &range.path)?
    };

    let cache_packer
        = package_cache.packer();

    let package_subdir
        = descriptor.ident.nm_subdir();

    tokio::task::spawn_blocking(move || -> Result<zpm_utils::Hash64, Error> {
        let entries
            = zpm_formats::entries_from_folder(&context_directory)?
                .into_iter()
                .prepare_npm_entries(&package_subdir)?;

        let archive
            = cache_packer.pack(entries)?;

        let mut writer
            = Hash64Writer::new();
        writer.update(b"file-folder-v2");
        writer.update(archive);

        Ok(writer.finalize())
    }).await?
}

pub async fn resolve_locator(context: &InstallContext<'_>, locator: &Locator, _params: &FolderReference, dependencies: Vec<InstallOpResult>) -> Result<ResolutionResult, Error> {
    let fetch_result
        = fetchers::fetch_locator(context.clone(), locator, false, dependencies).await?;

    fetch_result.into_resolution_result(context)
}
