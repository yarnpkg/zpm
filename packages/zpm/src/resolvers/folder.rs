use zpm_primitives::{Descriptor, FolderRange, FolderReference, Locator};

use crate::{
    error::Error,
    fetchers,
    install::{InstallContext, InstallOpResult, IntoResolutionResult, ResolutionResult},
};

pub async fn resolve_descriptor(context: &InstallContext<'_>, descriptor: &Descriptor, range: &FolderRange, dependencies: Vec<InstallOpResult>) -> Result<ResolutionResult, Error> {
    let locator = descriptor.resolve_with(FolderReference {
        path: range.path.to_string(),
    }.into());

    let fetch_result
        = fetchers::fetch_locator(context.clone(), &locator, false, dependencies).await?;

    Ok(fetch_result.into_resolution_result(context))
}

pub async fn resolve_locator(context: &InstallContext<'_>, locator: &Locator, _params: &FolderReference, dependencies: Vec<InstallOpResult>) -> Result<ResolutionResult, Error> {
    let fetch_result
        = fetchers::fetch_locator(context.clone(), locator, false, dependencies).await?;

    Ok(fetch_result.into_resolution_result(context))
}
