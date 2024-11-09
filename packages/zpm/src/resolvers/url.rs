use crate::{error::Error, fetchers, install::{InstallContext, IntoResolutionResult, ResolutionResult}, primitives::{range, reference, Descriptor, Locator}};

pub async fn resolve_descriptor(context: &InstallContext<'_>, descriptor: &Descriptor, params: &range::UrlRange) -> Result<ResolutionResult, Error> {
    let reference = reference::UrlReference {
        url: params.url.clone(),
    };

    let locator = descriptor.resolve_with(reference.into());

    let fetch_result
        = fetchers::fetch_locator(context.clone(), &locator, false, vec![]).await?;

    Ok(fetch_result.into_resolution_result(context))
}

pub async fn resolve_locator(context: &InstallContext<'_>, locator: &Locator, _params: &reference::UrlReference) -> Result<ResolutionResult, Error> {
    let fetch_result
        = fetchers::fetch_locator(context.clone(), &locator, false, vec![]).await?;

    Ok(fetch_result.into_resolution_result(context))
}
