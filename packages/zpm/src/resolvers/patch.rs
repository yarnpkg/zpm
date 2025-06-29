use crate::{error::Error, fetchers, install::{InstallContext, InstallOpResult, IntoResolutionResult, ResolutionResult}, primitives::{range, reference, Descriptor, Locator}, serialize::UrlEncoded};

pub async fn resolve_descriptor(context: &InstallContext<'_>, descriptor: &Descriptor, params: &range::PatchRange, mut dependencies: Vec<InstallOpResult>) -> Result<ResolutionResult, Error> {
    let mut dependencies_it
        = dependencies.iter();

    // Patch dependencies don't always need to be bound to their parents; they only have the parent listed as dependency
    // if it's required (ie. the patchfile is a relative path from the parent package).
    if descriptor.range.must_bind() {
        dependencies_it.next().unwrap();
    }

    let inner_locator
        = dependencies_it.next().unwrap().as_resolved_locator().clone();

    let reference = reference::PatchReference {
        inner: Box::new(UrlEncoded::new(inner_locator)),
        path: params.path.clone(),
        checksum: None,
    };

    let locator
        = descriptor.resolve_with(reference.into());

    // We need to remove the "resolve" operation where we resolved the
    // descriptor into a locator before passing it to fetch
    dependencies.remove(match descriptor.range.must_bind() {
        true => 1,
        false => 0,
    });

    // TODO: The `dependencies` parameter shouldn't be empty (for example what about a patch of a patch?)
    let fetch_result
        = fetchers::fetch_locator(context.clone(), &locator, false, dependencies).await?;

    Ok(fetch_result.into_resolution_result(context))
}

pub async fn resolve_locator(context: &InstallContext<'_>, locator: &Locator, _params: &reference::PatchReference, dependencies: Vec<InstallOpResult>) -> Result<ResolutionResult, Error> {
    let fetch_result
        = fetchers::fetch_locator(context.clone(), locator, false, dependencies).await?;

    Ok(fetch_result.into_resolution_result(context))
}
