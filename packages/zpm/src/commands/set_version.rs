use clipanion::cli;
use zpm_parsers::{Document, JsonDocument, Value};
use zpm_switch::{PackageManagerField, PackageManagerReference, VersionPackageManagerReference};
use zpm_utils::{Path, ToFileString, ToHumanString};

use crate::error::Error;

/// Set the Yarn version used by the local project
///
/// This command updates the top-level `packageManager` field to point to the specified Yarn selector.
///
/// It never writes the deprecated `yarnPath` field.
///
#[cli::command]
#[cli::path("set", "version")]
#[cli::category("Configuration commands")]
pub struct SetVersion {
    /// Yarn version, channel, release line, or selector to store
    version: zpm_switch::Selector,
}

impl SetVersion {
    pub async fn execute(&self) -> Result<(), Error> {
        let cwd = Path::current_dir()?;
        let detected_root_path = zpm_switch::resolve_detected_root(&cwd)
            .map_err(|_| Error::FailedToGetSwitchDetectedRoot)?;

        let manifest_path = detected_root_path
            .with_join_str("package.json");

        let manifest_content = manifest_path
            .fs_read_prealloc()?;

        let mut document
            = JsonDocument::new(manifest_content)?;

        let resolved_version
            = zpm_switch::resolve_selector(&self.version).await?;

        let reference: PackageManagerReference = VersionPackageManagerReference {
            version: resolved_version.clone(),
        }.into();

        let package_manager
            = PackageManagerField::new_yarn(reference);

        document.set_path(
            &zpm_parsers::Path::from_segments(vec!["packageManager".to_string()]),
            Value::String(package_manager.to_file_string()),
        )?;

        manifest_path
            .fs_change(&document.input, false)?;

        println!("Switching to {}", resolved_version.to_print_string());
        println!("Saved into {}", manifest_path.to_print_string());

        Ok(())
    }
}
