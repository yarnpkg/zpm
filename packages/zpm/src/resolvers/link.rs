use std::collections::{HashMap, HashSet};

use crate::{error::Error, install::{InstallContext, IntoResolutionResult, ResolutionResult}, primitives::{range::LinkRange, reference, Descriptor}, resolvers::Resolution, semver, system};

pub fn resolve_descriptor(ctx: &InstallContext<'_>, descriptor: &Descriptor, params: &LinkRange) -> Result<ResolutionResult, Error> {
    let reference = reference::LinkReference {
        path: params.path.clone(),
    };

    let locator = descriptor.resolve_with(reference.into());

    let resolution = Resolution {
        version: semver::Version::new(),
        locator,
        dependencies: HashMap::new(),
        peer_dependencies: HashMap::new(),
        optional_dependencies: HashSet::new(),
        missing_peer_dependencies: HashSet::new(),
        requirements: system::Requirements::default(),
    };

    Ok(resolution.into_resolution_result(ctx))
}
