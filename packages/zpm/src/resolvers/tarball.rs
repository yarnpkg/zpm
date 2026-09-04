use zpm_primitives::{Descriptor, Locator, TarballRange, TarballReference};
use zpm_utils::{FromFileString, Hash64, Path};

use crate::{
    error::Error,
    fetchers,
    install::{InstallContext, InstallOpResult, IntoResolutionResult, ResolutionResult},
};

pub async fn resolve_descriptor(context: &InstallContext<'_>, descriptor: &Descriptor, params: &TarballRange, dependencies: Vec<InstallOpResult>) -> Result<ResolutionResult, Error> {
    let hash
        = compute_tarball_hash(context, params, &dependencies)?;

    let locator = descriptor.resolve_with(TarballReference {
        path: params.path.clone(),
        hash: Some(hash),
    }.into());

    let fetch_result
        = fetchers::fetch_locator(context.clone(), &locator, false, dependencies).await?;

    fetch_result.into_resolution_result(context)
}

fn compute_tarball_hash(context: &InstallContext<'_>, params: &TarballRange, dependencies: &[InstallOpResult]) -> Result<Hash64, Error> {
    let tarball_relative_path
        = Path::from_file_string(&params.path)?;

    let tarball_path = if tarball_relative_path.is_absolute() {
        context.absolute_source_path(&tarball_relative_path)?
    } else {
        let parent_data
            = dependencies.first()
                .ok_or(Error::Unsupported)?
                .as_fetched();

        context.relative_source_path(parent_data.package_data.context_directory(), &params.path)?
    };

    Ok(Hash64::from_data(tarball_path.fs_read()?))
}

pub async fn resolve_locator(context: &InstallContext<'_>, locator: &Locator, _params: &TarballReference, dependencies: Vec<InstallOpResult>) -> Result<ResolutionResult, Error> {
    let fetch_result
        = fetchers::fetch_locator(context.clone(), locator, false, dependencies).await?;

    fetch_result.into_resolution_result(context)
}
