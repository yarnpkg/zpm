use std::process::ExitStatus;

use clipanion::cli;
use zpm_utils::ToFileString;

use crate::{cwd::{get_fake_cwd, get_final_cwd}, errors::Error, links::{LinkTarget, get_link, unset_link}, manifest::{LocalPackageManagerReference, PackageManagerField, find_closest_package_manager}, yarn::get_default_yarn_version, yarn_enums::ReleaseLine};

use super::switch::explicit::ExplicitCommand;

/// Call the suitable Yarn binary for the current project
#[cli::command(default, proxy)]
#[cli::category("General commands")]
#[derive(Debug)]
pub struct ProxyCommand {
    args: Vec<String>,
}

impl ProxyCommand {
    pub async fn execute(&self) -> Result<ExitStatus, Error> {
        let lookup_path
            = get_final_cwd()?;

        let mut find_result
            = find_closest_package_manager(&lookup_path)?;

        if let Some(detected_root_path) = find_result.detected_root_path {
            std::env::set_var("YARNSW_DETECTED_ROOT", detected_root_path.to_file_string());

            if let Some(link) = get_link(&detected_root_path)? {
                match link.link_target {
                    LinkTarget::Local {bin_path} => {
                        find_result.detected_package_manager
                            = Some(PackageManagerField::new_yarn(LocalPackageManagerReference {path: bin_path}.into()));
                    },

                    LinkTarget::Migration => {
                        if let Some(migration) = find_result.detected_package_manager_migration {
                            std::env::set_var("YARN_ENABLE_MIGRATION_MODE", "1");

                            find_result.detected_package_manager
                                = Some(PackageManagerField::new_yarn(migration.into_reference("yarn")?));
                        } else {
                            unset_link(&detected_root_path)?;
                        }
                    },
                };
            }
        }

        let reference = match find_result.detected_package_manager {
            Some(package_manager) => package_manager.into_reference("yarn"),
            None => get_default_yarn_version(Some(ReleaseLine::Classic)).await,
        }?;

        let mut args
            = self.args.clone();

        // Don't forget to add back the cwd parameter that was removed earlier on!
        if let Some(cwd) = get_fake_cwd() {
            args.insert(0, cwd.to_file_string());
        }

        ExplicitCommand::run(&reference, &args).await
    }
}
