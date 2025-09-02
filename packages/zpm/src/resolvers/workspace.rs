use zpm_primitives::{Descriptor, Ident, Locator, Reference, WorkspaceIdentRange, WorkspaceIdentReference, WorkspacePathRange, WorkspacePathReference};

use crate::{
    error::Error,
    install::{InstallContext, IntoResolutionResult, ResolutionResult},
    resolvers::Resolution,
};

pub fn resolve_name_descriptor(context: &InstallContext<'_>, descriptor: &Descriptor, params: &WorkspaceIdentRange) -> Result<ResolutionResult, Error> {
    let project = context.project
        .expect("The project is required for resolving a workspace package");

    let manifest = project.workspace_by_ident(&params.ident)?
        .manifest
        .clone();

    let reference = WorkspaceIdentReference {
        ident: params.ident.clone(),
    };

    let locator
        = descriptor.resolve_with(reference.into());
    let mut resolution
        = Resolution::from_remote_manifest(locator, manifest.remote);

    resolution.dependencies.extend(manifest.dev_dependencies);

    Ok(resolution.into_resolution_result(context))
}

pub fn resolve_path_descriptor(context: &InstallContext<'_>, descriptor: &Descriptor, params: &WorkspacePathRange) -> Result<ResolutionResult, Error> {
    let project = context.project
        .expect("The project is required for resolving a workspace package");

    let workspace
        = project.workspace_by_rel_path(&params.path)?;

    resolve_name_descriptor(context, descriptor, &WorkspaceIdentRange {ident: workspace.name.clone()})
}

pub fn resolve_ident(context: &InstallContext<'_>, ident: &Ident) -> Option<ResolutionResult> {
    let project = context.project
        .expect("The project is required for resolving a workspace package");

    let Ok(workspace) = project.workspace_by_ident(&ident) else {
        return None;
    };

    let locator = Locator::new(ident.clone(), WorkspaceIdentReference {
        ident: workspace.name.clone(),
    }.into());

    let Reference::WorkspaceIdent(reference) = &locator.reference else {
        panic!("Expected the locator to be a workspace ident");
    };

    let resolved
        = resolve_locator_ident(context, &locator, reference)
            .expect("Expected the locator to be resolved");

    Some(resolved)
}

pub fn resolve_locator_ident(context: &InstallContext<'_>, locator: &Locator, params: &WorkspaceIdentReference) -> Result<ResolutionResult, Error> {
    let project = context.project
        .expect("The project is required for resolving a workspace package");

    let manifest = project
        .workspace_by_ident(&params.ident)?
        .manifest
        .clone();

    let mut resolution
        = Resolution::from_remote_manifest(locator.clone(), manifest.remote);

    resolution.dependencies.extend(manifest.dev_dependencies);

    Ok(resolution.into_resolution_result(context))
}

pub fn resolve_locator_path(context: &InstallContext<'_>, locator: &Locator, params: &WorkspacePathReference) -> Result<ResolutionResult, Error> {
    let project = context.project
        .expect("The project is required for resolving a workspace package");

    let manifest = project
        .workspace_by_rel_path(&params.path)?
        .manifest
        .clone();

    let mut resolution
        = Resolution::from_remote_manifest(locator.clone(), manifest.remote);

    resolution.dependencies.extend(manifest.dev_dependencies);

    Ok(resolution.into_resolution_result(context))
}
