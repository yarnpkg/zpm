use zpm_primitives::{Descriptor, ExecRange, ExecReference, Locator};

use crate::{
    error::Error,
    fetchers,
    install::{InstallContext, InstallOpResult, IntoResolutionResult, ResolutionResult},
};

pub async fn resolve_descriptor(context: &InstallContext<'_>, descriptor: &Descriptor, params: &ExecRange, dependencies: Vec<InstallOpResult>) -> Result<ResolutionResult, Error> {
    let locator = descriptor.resolve_with(ExecReference {
        path: params.path.clone(),
    }.into());

    let fetch_result
        = fetchers::fetch_locator(context.clone(), &locator, false, dependencies).await?;

    fetch_result.into_resolution_result(context)
}

pub async fn resolve_locator(context: &InstallContext<'_>, locator: &Locator, _params: &ExecReference, dependencies: Vec<InstallOpResult>) -> Result<ResolutionResult, Error> {
    let fetch_result
        = fetchers::fetch_locator(context.clone(), locator, false, dependencies).await?;

    fetch_result.into_resolution_result(context)
}
