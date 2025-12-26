use zpm_primitives::{BuiltinRange, Descriptor, Locator};

use crate::{
    builtins, error::Error, install::{InstallContext, ResolutionResult}
};

pub async fn resolve_builtin_descriptor(context: &InstallContext<'_>, descriptor: &Descriptor, params: &BuiltinRange) -> Result<ResolutionResult, Error> {
    if descriptor.ident.as_str().starts_with("@builtin/node-") {
        return builtins::node::resolve_nodejs_variant_descriptor(context, descriptor, &params.range).await;
    }

    match descriptor.ident.as_str() {
        "@builtin/node"
            => builtins::node::resolve_nodejs_descriptor(context, descriptor, params).await,

        _ => Err(Error::Unsupported)?,
    }
}

pub async fn resolve_builtin_locator(context: &InstallContext<'_>, locator: &Locator, version: &zpm_semver::Version) -> Result<ResolutionResult, Error> {
    if locator.ident.as_str().starts_with("@builtin/node-") {
        return builtins::node::resolve_nodejs_variant_locator(context, locator, version).await;
    }

    match locator.ident.as_str() {
        _
            => Err(Error::Unsupported)?,
    }
}
