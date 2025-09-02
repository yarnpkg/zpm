use zpm_primitives::{AnonymousSemverRange, Descriptor, RegistrySemverRange, WorkspaceIdentRange};

use crate::{
    error::Error,
    install::{InstallContext, ResolutionResult},
    resolvers::{npm, workspace},
};

pub async fn resolve_descriptor(context: &InstallContext<'_>, descriptor: &Descriptor, params: &AnonymousSemverRange) -> Result<ResolutionResult, Error> {
    let project = context.project
        .expect("The project is required for resolving an anonymous semver range");

    if project.config.settings.enable_transparent_workspaces.value {
        if let Ok(workspace) = workspace::resolve_name_descriptor(context, descriptor, &WorkspaceIdentRange {ident: descriptor.ident.clone()}) {
            if params.range.check(&workspace.resolution.version) {
                return Ok(workspace);
            }
        }
    }

    npm::resolve_semver_descriptor(context, descriptor, &RegistrySemverRange {
        ident: None,
        range: params.range.clone(),
    }).await
}
