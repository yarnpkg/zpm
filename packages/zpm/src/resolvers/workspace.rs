use arca::Path;

use crate::{error::Error, install::{InstallContext, IntoResolutionResult, ResolutionResult}, primitives::{range, reference, Descriptor}, resolvers::Resolution};

pub fn resolve_name_descriptor(context: &InstallContext<'_>, descriptor: &Descriptor, params: &range::WorkspaceIdentRange) -> Result<ResolutionResult, Error> {
    let project = context.project
        .expect("The project is required for resolving a workspace package");

    let workspace = project.workspaces.get(&params.ident)
        .ok_or_else(|| Error::WorkspaceNotFound(params.ident.clone()))?;

    let manifest = workspace.manifest.clone();

    let reference = reference::WorkspaceReference {
        ident: params.ident.clone(),
    };

    let locator = descriptor.resolve_with(reference.into());
    let mut resolution = Resolution::from_remote_manifest(locator, manifest.remote);

    resolution.dependencies.extend(manifest.dev_dependencies);

    Ok(resolution.into_resolution_result(context))
}

pub fn resolve_path_descriptor(context: &InstallContext<'_>, descriptor: &Descriptor, params: &range::WorkspacePathRange) -> Result<ResolutionResult, Error> {
    let project = context.project
        .expect("The project is required for resolving a workspace package");

    if let Some(ident) = project.workspaces_by_rel_path.get(&Path::from(&params.path)) {
        resolve_name_descriptor(context, descriptor, &range::WorkspaceIdentRange {ident: ident.clone()})
    } else {
        Err(Error::WorkspacePathNotFound())
    }
}
