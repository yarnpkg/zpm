use std::collections::BTreeSet;

use clipanion::cli;
use serde::Serialize;
use zpm_formats::iter_ext::IterExt;
use zpm_parsers::JsonDocument;
use zpm_primitives::{Ident, Locator, Reference};
use zpm_utils::{DataType, Path, ToFileString, ToHumanString};

use crate::{error::Error, fetchers::PackageData, install::InstallResult, project::{self, Project, RunInstallOptions}};

/// Start writing a patch for the package
///
/// This command will cause a package to be extracted in a temporary directory intended to be editable at will.
///
/// Once you're done with your changes, run `yarn patch-commit -s path` (with `path` being the temporary directory you received) to generate a
/// patchfile and register it into your top-level manifest via the `patch:` protocol. Run `yarn patch-commit -h` for more details.
///
/// Calling the command when you already have a patch won't import it by default (in other words, the default behavior is to reset existing
/// patches). However, adding the `-u,--update` flag will import any current patch.
///
#[cli::command]
#[cli::path("patch")]
#[cli::category("Dependency management")]
pub struct Patch {
    /// Reapply local patches that already apply to this packages
    #[cli::option("-u,--update", default = false)]
    update: bool,

    /// Format the output as an NDJSON stream
    #[cli::option("--json", default = false)]
    json: bool,

    /// Package to patch
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

        let locator
            = Self::find_closest_dependency(&project, &self.ident)?
                .ok_or_else(|| Error::PackageNotFound(self.ident.clone()))?;

        let original_locator = if let Reference::Patch(params) = &locator.reference {
            &params.inner.0
        } else {
            &locator
        };

        let user_locator = if self.update {
            &locator
        } else {
            &original_locator
        };

        let root_path
            = Path::temp_dir_pattern("patch-<>")?;

        let locator_path = root_path
            .with_join_str(".locator");
        let original_path = root_path
            .with_join_str("original");
        let user_path = root_path
            .with_join_str("user");

        locator_path
            .fs_write(original_locator.to_file_string())?;

        Self::unpack_package_to(&install_result, &original_locator, &original_path)?;
        Self::unpack_package_to(&install_result, &user_locator, &user_path)?;

        if self.json {
            #[derive(Debug, Serialize)]
            struct PatchInfo<'a> {
                locator: &'a Locator,
                path: &'a Path,
            }

            println!("{}", JsonDocument::to_string(&PatchInfo {
                locator: &original_locator,
                path: &user_path,
            })?);
        } else {
            println!("Package {} got extracted with success!", original_locator.to_print_string());
            println!("You can now edit the following folder: {}", user_path.to_print_string());
            println!("Once you are done run {} and Yarn will store a patchfile based on your changes.", DataType::Code.colorize("yarn patch-commit -s PATCH_PATH"));
        }

        Ok(())
    }

    fn find_closest_dependency(project: &Project, searched_ident: &Ident) -> Result<Option<Locator>, Error> {
        let install_state = project.install_state.as_ref()
            .ok_or(Error::InstallStateNotFound)?;

        let mut lookup_search = vec![
            project.active_workspace()?.locator(),
        ];

        let mut seen
            = BTreeSet::from_iter(lookup_search.clone());

        while let Some(parent_locator) = lookup_search.pop() {
            let resolution
                = install_state.normalized_resolutions.get(&parent_locator)
                    .expect("Expected resolution for locator");

            for dependency in resolution.dependencies.values() {
                let dependency_locator
                    = install_state.descriptor_to_locator.get(&dependency)
                        .expect("Expected locator for descriptor");

                if seen.insert(dependency_locator.clone()) {
                    if &dependency_locator.ident == searched_ident {
                        return Ok(Some(dependency_locator.clone()));
                    }

                    lookup_search.push(dependency_locator.clone());
                }
            }
        }

        for locator in install_state.normalized_resolutions.keys() {
            if &locator.ident == searched_ident {
                return Ok(Some(locator.clone()));
            }
        }

        Ok(None)
    }

    fn unpack_package_to(install_result: &InstallResult, locator: &Locator, destination: &Path) -> Result<(), Error> {
        let Some(package_data) = install_result.package_data.get(&locator) else {
            return Ok(());
        };

        let PackageData::Zip {archive_path, package_directory, ..} = package_data else {
            return Ok(());
        };

        let archive_data = archive_path
            .fs_read_prealloc()?;

        let package_subdir
            = package_directory
                .strip_prefix(archive_path)
                .expect("Failed to strip prefix");

        let entries
            = zpm_formats::zip::entries_from_zip(&archive_data)?
                .into_iter()
                .strip_path_prefix(&package_subdir)
                .collect::<Vec<_>>();

        destination
            .fs_create_dir_all()?;

        zpm_formats::entries_to_disk(&entries, &destination)?;

        Ok(())
    }
}
