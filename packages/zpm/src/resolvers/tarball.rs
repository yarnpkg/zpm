use crate::{error::Error, fetchers, install::{InstallContext, InstallOpResult, IntoResolutionResult, ResolutionResult}, primitives::{range, reference, Descriptor, Locator}};

pub async fn resolve_descriptor(context: &InstallContext<'_>, descriptor: &Descriptor, params: &range::TarballRange, dependencies: Vec<InstallOpResult>) -> Result<ResolutionResult, Error> {
    let locator = descriptor.resolve_with(reference::TarballReference {
        path: params.path.clone(),
    }.into());

    let fetch_result
        = fetchers::fetch_locator(context.clone(), &locator, false, dependencies).await?;

    Ok(fetch_result.into_resolution_result(context))
}

pub async fn resolve_locator(context: &InstallContext<'_>, locator: &Locator, _params: &reference::TarballReference, dependencies: Vec<InstallOpResult>) -> Result<ResolutionResult, Error> {
    let fetch_result
        = fetchers::fetch_locator(context.clone(), &locator, false, dependencies).await?;

    Ok(fetch_result.into_resolution_result(context))
}
