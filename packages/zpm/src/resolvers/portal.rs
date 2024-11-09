use crate::{error::Error, formats::zip::ZipSupport, install::{InstallContext, InstallOpResult, IntoResolutionResult, ResolutionResult}, manifest::parse_manifest, primitives::{range, reference, Descriptor, Locator, Reference}, resolvers::Resolution};

pub fn resolve_descriptor(ctx: &InstallContext, descriptor: &Descriptor, params: &range::PortalRange, dependencies: Vec<InstallOpResult>) -> Result<ResolutionResult, Error> {
    let reference = reference::PortalReference {
        path: params.path.clone(),
    };

    let locator
        = descriptor.resolve_with(reference.into());

    let Reference::Portal(params) = &locator.reference else {
        unreachable!()
    };

    resolve_locator(ctx, &locator, params, dependencies)
}

pub fn resolve_locator(context: &InstallContext, locator: &Locator, params: &reference::PortalReference, dependencies: Vec<InstallOpResult>) -> Result<ResolutionResult, Error> {
    let parent_data = dependencies[0].as_fetched();

    let package_directory = parent_data.package_data
        .context_directory()
        .with_join_str(params.path.clone());

    let manifest_path = package_directory
        .with_join_str("package.json");
    let manifest_text = manifest_path
        .fs_read_text_with_zip()?;
    let manifest
        = parse_manifest(manifest_text)?;

    let resolution
        = Resolution::from_remote_manifest(locator.clone(), manifest.remote);

    Ok(resolution.into_resolution_result(context))
}
