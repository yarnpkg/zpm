use std::collections::{HashMap, HashSet};

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
        dependencies: HashMap::new(),
        peer_dependencies: HashMap::new(),
        optional_dependencies: HashSet::new(),
        missing_peer_dependencies: HashSet::new(),
        requirements: system::Requirements::default(),
    };

    Ok(resolution.into_resolution_result(ctx))
}
