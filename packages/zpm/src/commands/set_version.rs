use clipanion::cli;
use zpm_parsers::{JsonDocument, Value};
use zpm_switch::{PackageManagerField, PackageManagerReference, VersionPackageManagerReference};
use zpm_utils::{Path, ToFileString, ToHumanString};

use crate::error::Error;

/// Set the version of Yarn to use with the local project
#[cli::command]
#[cli::path("set", "version")]
#[cli::category("Configuration commands")]
pub struct SetVersion {
    version: zpm_switch::Selector,
}

impl SetVersion {
    pub async fn execute(&self) -> Result<(), Error> {
        let Ok(switch_detected_root) = std::env::var("YARNSW_DETECTED_ROOT") else {
            return Err(Error::FailedToGetSwitchDetectedRoot);
        };

        let detected_root_path
            = Path::try_from(&switch_detected_root)?;

        let manifest_path = detected_root_path
            .with_join_str("package.json");

        let manifest_content = manifest_path
            .fs_read_prealloc()?;

        let mut formatter
            = JsonDocument::new(manifest_content)?;

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

        formatter.set_path(
            &zpm_parsers::Path::from_segments(vec!["packageManager".to_string()]),
            Value::String(package_manager.to_file_string()),
        )?;

        manifest_path
            .fs_change(&formatter.input, false)?;

        println!("Switching to {}", resolved_version.to_print_string());
        println!("Saved into {}", manifest_path.to_print_string());

        Ok(())
    }
}
