use zpm_primitives::{Descriptor, Ident, JsrSemverRange, JsrTagRange, RegistrySemverRange, RegistryTagRange};

use crate::{
    error::Error,
    install::{InstallContext, InstallOpResult, ResolutionResult},
    resolvers::npm,
};

pub fn npm_ident_for_jsr_ident(ident: &Ident) -> Ident {
    ident.scope().map_or_else(
        || Ident::new(format!("@jsr/{}", ident.name())),
        |scope| Ident::new(format!("@jsr/{}__{}", scope.trim_start_matches('@'), ident.name())),
    )
}

pub fn resolve_aliased(descriptor: &Descriptor, dependencies: Vec<InstallOpResult>) -> Result<ResolutionResult, Error> {
    npm::resolve_aliased(descriptor, dependencies)
}

pub async fn resolve_semver_descriptor(context: &InstallContext<'_>, descriptor: &Descriptor, params: &JsrSemverRange) -> Result<ResolutionResult, Error> {
    let package_ident
        = npm_ident_for_jsr_ident(params.ident.as_ref().unwrap_or(&descriptor.ident));

    let registry_params = RegistrySemverRange {
        ident: Some(package_ident),
        range: params.range.clone(),
    };

    npm::resolve_semver_descriptor(context, descriptor, &registry_params).await
}

pub async fn resolve_tag_descriptor(context: &InstallContext<'_>, descriptor: &Descriptor, params: &JsrTagRange) -> Result<ResolutionResult, Error> {
    let package_ident
        = npm_ident_for_jsr_ident(params.ident.as_ref().unwrap_or(&descriptor.ident));

    let registry_params = RegistryTagRange {
        ident: Some(package_ident),
        tag: params.tag.clone(),
    };

    npm::resolve_tag_descriptor(context, descriptor, &registry_params).await
}
