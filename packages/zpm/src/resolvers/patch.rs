use crate::{error::Error, fetchers, install::{InstallContext, InstallOpResult, IntoResolutionResult, ResolutionResult}, primitives::{range::PatchRange, reference, Descriptor}, serialize::UrlEncoded};

pub async fn resolve_descriptor(context: &InstallContext<'_>, descriptor: &Descriptor, range: &PatchRange, mut dependencies: Vec<InstallOpResult>) -> Result<ResolutionResult, Error> {
    let inner_locator
        = dependencies[1].as_resolved().resolution.locator.clone();

    let reference = reference::PatchReference {
        inner: Box::new(UrlEncoded::new(inner_locator)),
        path: range.path.clone(),
    };

    let locator = descriptor.resolve_with(reference.into());

    // We need to remove the "resolve" operation where we resolved the
    // descriptor into a locator before passing it to fetch
    dependencies.remove(1);

    // TODO: The `dependencies` parameter shouldn't be empty (for example what about a patch of a patch?)
    let fetch_result
        = fetchers::fetch(context.clone(), &locator, false, dependencies).await?;

    Ok(fetch_result.into_resolution_result(context))
}
