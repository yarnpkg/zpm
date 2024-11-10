use crate::{error::Error, fetchers, install::{InstallContext, InstallOpResult, IntoResolutionResult, ResolutionResult}, primitives::{range, reference, Descriptor, Locator}, serialize::UrlEncoded};

pub async fn resolve_descriptor(context: &InstallContext<'_>, descriptor: &Descriptor, params: &range::PatchRange, mut dependencies: Vec<InstallOpResult>) -> Result<ResolutionResult, Error> {
    let inner_locator
        = dependencies[1].as_resolved().resolution.locator.clone();

    let reference = reference::PatchReference {
        inner: Box::new(UrlEncoded::new(inner_locator)),
        path: params.path.clone(),
    };

    let locator = descriptor.resolve_with(reference.into());

    // We need to remove the "resolve" operation where we resolved the
    // descriptor into a locator before passing it to fetch
    dependencies.remove(1);

    // TODO: The `dependencies` parameter shouldn't be empty (for example what about a patch of a patch?)
    let fetch_result
        = fetchers::fetch_locator(context.clone(), &locator, false, dependencies).await?;

    Ok(fetch_result.into_resolution_result(context))
}

pub async fn resolve_locator(context: &InstallContext<'_>, locator: &Locator, _params: &reference::PatchReference, dependencies: Vec<InstallOpResult>) -> Result<ResolutionResult, Error> {
    let fetch_result
        = fetchers::fetch_locator(context.clone(), &locator, false, dependencies).await?;

    Ok(fetch_result.into_resolution_result(context))
}
