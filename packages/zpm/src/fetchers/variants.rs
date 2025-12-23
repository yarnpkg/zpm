use zpm_primitives::Locator;

use crate::{error::Error, fetchers::PackageData, install::{FetchResult, InstallContext}};

pub fn fetch_locator<'a>(_context: &InstallContext<'a>, _locator: &Locator) -> Result<FetchResult, Error> {
    Ok(FetchResult {
        resolution: None,
        package_data: PackageData::Abstract,
    })
}
