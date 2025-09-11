use clipanion::cli;
use zpm_primitives::{Ident, Locator, Reference};
use zpm_utils::{DataType, Path, ToFileString, ToHumanString};

use crate::{error::Error, project};

#[cli::command]
#[cli::path("patch")]
#[cli::category("Dependency management")]
#[cli::description("Start writing a patch for the package")]
pub struct Patch {
    ident: Ident,
}

impl Patch {
    #[tokio::main()]
    pub async fn execute(&self) -> Result<(), Error> {
        let mut project
            = project::Project::new(None).await?;

        project
            .lazy_install().await?;

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

        let project_cache
            = project.package_cache()?;

        for locator in relevant_locators {
            let cache_path
                = project_cache.key_path(&locator, ".zip");

            if !cache_path.fs_exists() {
                continue;
            }

            let cache_data
                = cache_path.fs_read()?;

            let entries
                = zpm_formats::zip::entries_from_zip(&cache_data)?;

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
