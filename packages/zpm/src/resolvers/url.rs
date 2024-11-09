use crate::{error::Error, fetchers, install::{InstallContext, IntoResolutionResult, ResolutionResult}, primitives::{range, reference, Descriptor}};

pub async fn resolve_descriptor(context: &InstallContext<'_>, descriptor: &Descriptor, params: &range::UrlRange) -> Result<ResolutionResult, Error> {
    let reference = reference::UrlReference {
        url: params.url.clone(),
    };

    let locator = descriptor.resolve_with(reference.into());

    let fetch_result
        = fetchers::fetch(context.clone(), &locator, false, vec![]).await?;

    Ok(fetch_result.into_resolution_result(context))
}
