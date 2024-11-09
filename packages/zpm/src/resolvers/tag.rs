use crate::{error::Error, install::{InstallContext, ResolutionResult}, primitives::{range::{self, WorkspaceIdentRange}, Descriptor}};

use super::{npm, workspace};

pub async fn resolve_descriptor(context: &InstallContext<'_>, descriptor: &Descriptor, params: &range::AnonymousTagRange) -> Result<ResolutionResult, Error> {
    let project = context.project
        .expect("The project is required for resolving an anonymous tag");

    if project.config.project.enable_transparent_workspaces.value {
        if let Ok(workspace) = workspace::resolve_name_descriptor(context, &descriptor, &WorkspaceIdentRange {ident: descriptor.ident.clone()}) {
            return Ok(workspace);
        }
    }

    npm::resolve_tag_descriptor(context, descriptor, &&range::RegistryTagRange {
        ident: None,
        tag: params.tag.clone(),
    }.into()).await
}
