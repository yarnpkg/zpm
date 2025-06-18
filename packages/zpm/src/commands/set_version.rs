use std::{fs::Permissions, os::unix::fs::PermissionsExt};

use clipanion::cli;
use zpm_parsers::{JsonFormatter, JsonValue};
use zpm_switch::{PackageManagerField, PackageManagerReference, VersionPackageManagerReference};
use zpm_utils::{Path, ToHumanString};

use crate::error::Error;

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

        let manifest_content = manifest_path
            .fs_read_text_prealloc()?;

        let mut formatter
            = JsonFormatter::from(&manifest_content).unwrap();

        let resolved_version
            = zpm_switch::resolve_selector(&self.version).await?;

        let reference: PackageManagerReference = VersionPackageManagerReference {
            version: resolved_version.clone(),
        }.into();

        let package_manager = PackageManagerField {
            name: "yarn".to_string(),
            reference,
            checksum: None,
        };

        formatter.set(
            &vec!["packageManager".to_string()].into(),
            JsonValue::String(package_manager.to_string()),
        ).unwrap();

        let updated_content
            = formatter.to_string();

        manifest_path
            .fs_change(&updated_content, Permissions::from_mode(0o644))?;

        println!("Switching to {}", resolved_version.to_print_string());
        println!("Saved into {}", manifest_path.to_print_string());

        Ok(())
    }
}
