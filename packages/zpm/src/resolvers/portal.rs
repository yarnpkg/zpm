use zpm_formats::zip::ZipSupport;
use zpm_primitives::{Descriptor, Locator, PortalRange, PortalReference, Reference};

use crate::{
    error::Error,
    install::{InstallContext, InstallOpResult, IntoResolutionResult, ResolutionResult},
    manifest::helpers::parse_manifest,
    resolvers::Resolution,
};

pub fn resolve_descriptor(ctx: &InstallContext, descriptor: &Descriptor, params: &PortalRange, dependencies: Vec<InstallOpResult>) -> Result<ResolutionResult, Error> {
    let reference = PortalReference {
        path: params.path.clone(),
    };

    let locator
        = descriptor.resolve_with(reference.into());

    let Reference::Portal(params) = &locator.reference else {
        unreachable!()
    };

    resolve_locator(ctx, &locator, params, dependencies)
}

pub fn resolve_locator(context: &InstallContext, locator: &Locator, params: &PortalReference, dependencies: Vec<InstallOpResult>) -> Result<ResolutionResult, Error> {
    let parent_data
        = dependencies[0].as_fetched();

    let package_directory = parent_data.package_data
        .context_directory()
        .with_join_str(params.path.clone());

    let manifest_path = package_directory
        .with_join_str("package.json");
    let manifest_text = manifest_path
        .fs_read_text_with_zip()?;
    let manifest
        = parse_manifest(&manifest_text)?;

    let resolution
        = Resolution::from_remote_manifest(locator.clone(), manifest.remote);

    Ok(resolution.into_resolution_result(context))
}
