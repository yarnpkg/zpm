use std::{fs::Permissions, os::unix::fs::PermissionsExt};

use clipanion::cli;
use zpm_switch::{PackageManagerField, PackageManagerReference, VersionPackageManagerReference};
use zpm_utils::Path;

use crate::{error::Error, manifest::helpers::read_manifest};

#[cli::command]
#[cli::path("set", "version")]
#[cli::category("Configuration commands")]
#[cli::description("Set the version of Yarn to use with the local project")]
pub struct SetVersion {
    version: zpm_switch::Selector,
}

impl SetVersion {
    #[tokio::main()]
    pub async fn execute(&self) -> Result<(), Error> {
        let Ok(switch_detected_root) = std::env::var("YARNSW_DETECTED_ROOT") else {
            return Err(Error::FailedToGetSwitchDetectedRoot);
        };

        let detected_root_path
            = Path::try_from(&switch_detected_root)?;

        let manifest_path = detected_root_path
            .with_join_str("package.json");

        let mut manifest
            = read_manifest(&manifest_path)?;

        let resolved_version
            = zpm_switch::resolve_selector(&self.version).await?;

        let reference: PackageManagerReference = VersionPackageManagerReference {
            version: resolved_version,
        }.into();

        manifest.package_manager = Some(PackageManagerField {
            name: "yarn".to_string(),
            reference,
            checksum: None,
        });

        let serialized
            = sonic_rs::to_string_pretty(&manifest)?;

        manifest_path
            .fs_change(serialized, Permissions::from_mode(0o644))?;

        println!("{:#?}", manifest);

        println!("Saved into {}", manifest_path);

        Ok(())
    }
}
