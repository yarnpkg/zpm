use crate::{error::Error, install::{InstallContext, ResolutionResult}, primitives::{range::{self, WorkspaceIdentRange}, Descriptor}};

use super::{npm, workspace};

pub async fn resolve_descriptor(context: &InstallContext<'_>, descriptor: &Descriptor, params: &range::AnonymousSemverRange) -> Result<ResolutionResult, Error> {
    let project = context.project
        .expect("The project is required for resolving an anonymous semver range");

    if project.config.project.enable_transparent_workspaces.value {
        if let Ok(workspace) = workspace::resolve_name_descriptor(context, &descriptor, &WorkspaceIdentRange {ident: descriptor.ident.clone()}) {
            if params.range.check(&workspace.resolution.version) {
                return Ok(workspace);
            }
        }
    }

    npm::resolve_semver_descriptor(context, descriptor, &range::RegistrySemverRange {
        ident: None,
        range: params.range.clone(),
    }.into()).await
}
