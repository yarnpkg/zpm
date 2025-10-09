use clipanion::cli;
use zpm_formats::iter_ext::IterExt;
use zpm_primitives::{Ident, Locator, Reference};
use zpm_utils::{DataType, Path, ToFileString, ToHumanString};

use crate::{error::Error, fetchers::PackageData, project::{self, RunInstallOptions}};

/// Start writing a patch for the package
#[cli::command]
#[cli::path("patch")]
#[cli::category("Dependency management")]
pub struct Patch {
    ident: Ident,
}

impl Patch {
    pub async fn execute(&self) -> Result<(), Error> {
        let mut project
            = project::Project::new(None).await?;

        let install_result = project.run_install(RunInstallOptions {
            silent_or_error: true,
            ..Default::default()
        }).await?;

        let install_state = project.install_state.as_ref()
            .ok_or(Error::InstallStateNotFound)?;

        let unwrap_patch_locator = |locator: Locator| match locator.reference {
            Reference::Patch(params) => params.inner.clone().0,
            _ => locator,
        };

        let relevant_locators = install_state.normalized_resolutions.keys()
            .filter(|locator| locator.ident == self.ident)
            .cloned()
            .map(unwrap_patch_locator)
            .collect::<Vec<_>>();

        for locator in relevant_locators {
            let Some(package_data) = install_result.package_data.get(&locator) else {
                continue;
            };

            let PackageData::Zip {archive_path, package_directory, ..} = package_data else {
                continue;
            };

            let cache_data = archive_path
                .fs_read()?;

            let package_subdir
                = package_directory
                    .strip_prefix(archive_path)
                    .expect("The package directory should lead within the archive");

            let entries
                = zpm_formats::zip::entries_from_zip(&cache_data)?
                    .into_iter()
                    .strip_path_prefix(&package_subdir)
                    .collect::<Vec<_>>();

            let root_path
                = Path::temp_dir_pattern("patch-<>")?;

            let locator_path = root_path
                .with_join_str(".locator");

            locator_path
                .fs_write(locator.to_file_string())?;

            let original_path = root_path
                .with_join_str("original");
            let user_path = root_path
                .with_join_str("user");

            original_path
                .fs_create_dir_all()?;
            user_path
                .fs_create_dir_all()?;

            zpm_formats::entries_to_disk(&entries, &original_path)?;
            zpm_formats::entries_to_disk(&entries, &user_path)?;

            println!("Package {} got extracted with success!", locator.to_print_string());
            println!("You can now edit the following folder: {}", user_path.to_print_string());
            println!("Once you are done run {} and Yarn will store a patchfile based on your changes.", DataType::Code.colorize("yarn patch-commit -s PATCH_PATH"));
        }

        Ok(())
    }
}
