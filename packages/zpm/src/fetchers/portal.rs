use zpm_primitives::{Locator, PortalReference};
use zpm_utils::Path;

use crate::{
    error::Error,
    install::{FetchResult, InstallContext, InstallOpResult},
};

use super::PackageData;

pub fn fetch_locator(context: &InstallContext, _locator: &Locator, params: &PortalReference, dependencies: Vec<InstallOpResult>) -> Result<FetchResult, Error> {
    let parent_data
        = dependencies[0].as_fetched();

    let path
        = Path::try_from(params.path.as_str())?;
    let package_directory = if path.is_absolute() {
        context.absolute_source_path(&path)?
    } else {
        context.relative_source_path(parent_data.package_data.context_directory(), &params.path)?
    };

    Ok(FetchResult {
        resolution: None,
        package_data: PackageData::Local {
            package_directory,
            is_synthetic_package: false,
        },
    })
}
