use clipanion::cli;

use crate::{cwd::get_final_cwd, errors::Error, install::install_package_manager, manifest::{find_closest_package_manager, PackageManagerReference, VersionPackageManagerReference}};

/// Install the current project's Yarn version, or specific Yarn releases
#[cli::command]
#[cli::path("switch", "cache")]
#[cli::category("Cache management")]
#[derive(Debug)]
pub struct CacheInstallCommand {
    #[cli::option("-i,--install")]
    _install: bool,

    versions: Vec<zpm_semver::Version>,
}

impl CacheInstallCommand {
    pub async fn execute(&self) -> Result<(), Error> {
        if self.versions.is_empty() {
            let lookup_path
                = get_final_cwd()?;

            let find_result
                = find_closest_package_manager(&lookup_path)?;

            let package_manager_field
                = find_result.detected_package_manager
                    .ok_or_else(|| Error::ProjectNotFound)?;

            let reference
                = package_manager_field.into_reference("yarn")?;

            if let PackageManagerReference::Version(params) = reference {
                install_package_manager(&params).await?;
            }
        } else {
            for version in &self.versions {
                let params
                    = VersionPackageManagerReference {version: version.clone()};

                install_package_manager(&params.into()).await?;
            }
        }

        Ok(())
    }
}
