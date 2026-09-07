use zpm_formats::zip::ZipSupport;
use zpm_primitives::{Descriptor, Locator, PortalRange, PortalReference, Reference};
use zpm_utils::{Hash64, Path};

use crate::{
    error::Error,
    install::{InstallContext, InstallOpResult, IntoResolutionResult, ResolutionResult},
    manifest::helpers::parse_manifest,
    resolvers::Resolution,
};

/// Domain-separated hash of the portal target's manifest; shared with
/// the up-to-date fast path, which re-derives it to detect changes.
pub fn compute_portal_manifest_hash(manifest_text: &str) -> Hash64 {
    let mut writer = zpm_utils::Hash64Writer::new();
    writer.update(b"portal-manifest-v1");
    writer.update(manifest_text.as_bytes());

    writer.finalize()
}

fn portal_manifest_path(context_directory: &Path, portal_path: &str) -> Path {
    context_directory
        .with_join_str(portal_path)
        .with_join_str("package.json")
}

pub fn resolve_descriptor(ctx: &InstallContext, descriptor: &Descriptor, params: &PortalRange, dependencies: Vec<InstallOpResult>) -> Result<ResolutionResult, Error> {
    let parent_data
        = dependencies[0].as_fetched();

    let manifest_text = portal_manifest_path(parent_data.package_data.context_directory(), &params.path)
        .fs_read_text_with_zip()?;

    let reference = PortalReference {
        path: params.path.clone(),
        hash: Some(compute_portal_manifest_hash(&manifest_text)),
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

    let manifest_text = portal_manifest_path(parent_data.package_data.context_directory(), &params.path)
        .fs_read_text_with_zip()?;

    let manifest
        = parse_manifest(&manifest_text)?;

    let resolution
        = Resolution::from_remote_manifest(locator.clone(), manifest.remote);

    resolution.into_resolution_result(context)
}
