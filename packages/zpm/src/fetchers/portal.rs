use zpm_primitives::{Locator, PortalReference};

use crate::{
    error::Error,
    install::{FetchResult, InstallContext, InstallOpResult},
};

use super::PackageData;

pub fn fetch_locator(_context: &InstallContext, _locator: &Locator, params: &PortalReference, dependencies: Vec<InstallOpResult>) -> Result<FetchResult, Error> {
    let parent_data
        = dependencies[0].as_fetched();

    let package_directory = parent_data.package_data
        .context_directory()
        .with_join_str(&params.path);

    Ok(FetchResult {
        resolution: None,
        package_data: PackageData::Local {
            package_directory,
            is_synthetic_package: false,
        },
    })
}
