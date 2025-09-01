use zpm_primitives::{Descriptor, Locator, TarballRange, TarballReference};

use crate::{
    error::Error,
    fetchers,
    install::{InstallContext, InstallOpResult, IntoResolutionResult, ResolutionResult},
};

pub async fn resolve_descriptor(context: &InstallContext<'_>, descriptor: &Descriptor, params: &TarballRange, dependencies: Vec<InstallOpResult>) -> Result<ResolutionResult, Error> {
    let locator = descriptor.resolve_with(TarballReference {
        path: params.path.clone(),
    }.into());

    let fetch_result
        = fetchers::fetch_locator(context.clone(), &locator, false, dependencies).await?;

    Ok(fetch_result.into_resolution_result(context))
}

pub async fn resolve_locator(context: &InstallContext<'_>, locator: &Locator, _params: &TarballReference, dependencies: Vec<InstallOpResult>) -> Result<ResolutionResult, Error> {
    let fetch_result
        = fetchers::fetch_locator(context.clone(), locator, false, dependencies).await?;

    Ok(fetch_result.into_resolution_result(context))
}
