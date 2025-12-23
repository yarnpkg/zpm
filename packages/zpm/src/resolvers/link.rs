use zpm_primitives::{Descriptor, LinkRange, LinkReference, Locator, Reference};

use crate::{
    error::Error,
    install::{InstallContext, IntoResolutionResult, ResolutionResult},
    resolvers::Resolution,
};

pub fn resolve_descriptor(ctx: &InstallContext<'_>, descriptor: &Descriptor, params: &LinkRange) -> Result<ResolutionResult, Error> {
    let reference = LinkReference {
        path: params.path.clone(),
    };

    let locator
        = descriptor.resolve_with(reference.into());

    let Reference::Link(params) = &locator.reference else {
        unreachable!();
    };

    resolve_locator(ctx, &locator, params)
}

pub fn resolve_locator(ctx: &InstallContext<'_>, locator: &Locator, _params: &LinkReference) -> Result<ResolutionResult, Error> {
    let resolution
        = Resolution::new_empty(locator.clone(), zpm_semver::Version::new());

    Ok(resolution.into_resolution_result(ctx))
}
