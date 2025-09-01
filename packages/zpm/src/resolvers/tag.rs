use zpm_primitives::{AnonymousTagRange, Descriptor, RegistryTagRange, WorkspaceIdentRange};

use crate::{
    error::Error,
    install::{InstallContext, ResolutionResult},
    resolvers::{npm, workspace},
};

pub async fn resolve_descriptor(context: &InstallContext<'_>, descriptor: &Descriptor, params: &AnonymousTagRange) -> Result<ResolutionResult, Error> {
    let project = context.project
        .expect("The project is required for resolving an anonymous tag");

    if project.config.settings.enable_transparent_workspaces.value {
        if let Ok(workspace) = workspace::resolve_name_descriptor(context, descriptor, &WorkspaceIdentRange {ident: descriptor.ident.clone()}) {
            return Ok(workspace);
        }
    }

    npm::resolve_tag_descriptor(context, descriptor, &RegistryTagRange {
        ident: None,
        tag: params.tag.clone(),
    }).await
}
