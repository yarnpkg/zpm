use zpm_primitives::{Descriptor, Locator, PatchRange, PatchReference};
use zpm_utils::UrlEncoded;

use crate::{
    error::Error,
    fetchers,
    install::{InstallContext, InstallOpResult, IntoResolutionResult, ResolutionResult},
    primitives_exts::RangeExt,
};

pub async fn resolve_descriptor(context: &InstallContext<'_>, descriptor: &Descriptor, params: &PatchRange, mut dependencies: Vec<InstallOpResult>) -> Result<ResolutionResult, Error> {
    let mut dependencies_it
        = dependencies.iter();

    let range_details
        = descriptor.range.details();

    // Patch dependencies don't always need to be bound to their parents; they only have the parent listed as dependency
    // if it's required (ie. the patchfile is a relative path from the parent package).
    if range_details.require_binding {
        dependencies_it.next().unwrap();
    }

    let inner_locator
        = dependencies_it.next().unwrap().as_resolved_locator().clone();

    let reference = PatchReference {
        inner: Box::new(UrlEncoded::new(inner_locator.clone())),
        path: params.path.clone(),
        checksum: None,
    };

    let locator
        = descriptor.resolve_with(reference.into());

    // We need to remove the "resolve" operation where we resolved the
    // descriptor into a locator before passing it to fetch
    dependencies.remove(match range_details.require_binding {
        true => 1,
        false => 0,
    });

    // TODO: The `dependencies` parameter shouldn't be empty (for example what about a patch of a patch?)
    let fetch_result
        = fetchers::fetch_locator(context.clone(), &locator, false, dependencies).await?;

    Ok(fetch_result.into_resolution_result(context))
}

pub async fn resolve_locator(context: &InstallContext<'_>, locator: &Locator, _params: &PatchReference, dependencies: Vec<InstallOpResult>) -> Result<ResolutionResult, Error> {
    let fetch_result
        = fetchers::fetch_locator(context.clone(), locator, false, dependencies).await?;

    Ok(fetch_result.into_resolution_result(context))
}
