use zpm_primitives::{LinkReference, Locator};
use zpm_utils::{FromFileString, Path};

use crate::{
    error::Error,
    install::{FetchResult, InstallContext, InstallOpResult},
};

use super::PackageData;

pub fn fetch_locator(context: &InstallContext, _locator: &Locator, params: &LinkReference, dependencies: Vec<InstallOpResult>) -> Result<FetchResult, Error> {
    let link_relative_path
        = Path::from_file_string(&params.path)?;

    let package_directory = if link_relative_path.is_absolute() {
        context.absolute_source_path(&link_relative_path)?
    } else {
        let parent_data
            = dependencies.first()
                .ok_or(Error::Unsupported)?
                .as_fetched();

        context.relative_source_path(parent_data.package_data.context_directory(), &params.path)?
    };

    Ok(FetchResult {
        resolution: None,
        package_data: PackageData::Local {
            package_directory,
            is_synthetic_package: true,
        },
    })
}
