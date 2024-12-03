use std::collections::{BTreeMap, BTreeSet};

use crate::{error::Error, install::{InstallContext, IntoResolutionResult, ResolutionResult}, primitives::{range, reference, Descriptor, Locator, Reference}, resolvers::Resolution, semver, system};

pub fn resolve_descriptor(ctx: &InstallContext<'_>, descriptor: &Descriptor, params: &range::LinkRange) -> Result<ResolutionResult, Error> {
    let reference = reference::LinkReference {
        path: params.path.clone(),
    };

    let locator
        = descriptor.resolve_with(reference.into());

    let Reference::Link(params) = &locator.reference else {
        unreachable!();
    };

    resolve_locator(ctx, &locator, params)
}

pub fn resolve_locator(ctx: &InstallContext<'_>, locator: &Locator, _params: &reference::LinkReference) -> Result<ResolutionResult, Error> {
    let resolution = Resolution {
        version: semver::Version::new(),
        locator: locator.clone(),
        dependencies: BTreeMap::new(),
        peer_dependencies: BTreeMap::new(),
        optional_dependencies: BTreeSet::new(),
        missing_peer_dependencies: BTreeSet::new(),
        requirements: system::Requirements::default(),
    };

    Ok(resolution.into_resolution_result(ctx))
}
