use std::{collections::BTreeMap, io::Write, str::FromStr};

use clipanion::cli;
use itertools::Itertools;
use zpm_parsers::{JsonDocument, Value};
use zpm_primitives::{Descriptor, Locator, Range, Reference};
use zpm_utils::{FromFileString, Path, ToFileString, ToHumanString, UrlEncoded};

use crate::{error::Error, git, project};

/// Commit a patch for the package
#[derive(Debug)]
#[cli::command]
#[cli::path("patch-commit")]
#[cli::category("Dependency management")]
pub struct PatchCommit {
    #[cli::option("-s,--save", default = false)]
    save: bool,

    source: Path,
}

impl PatchCommit {
    pub async fn execute(&self) -> Result<(), Error> {
        let mut project
            = project::Project::new(None).await?;

        let locator_path = self.source
            .iter_path()
            .map(|path| path.with_join_str(".locator"))
            .find(|path| path.fs_exists())
            .ok_or_else(|| Error::NotAPatchFolder(self.source.clone()))?;

        let original_path = locator_path
            .dirname()
            .unwrap()
            .with_join_str("original");
        let user_path = self.source
            .dirname()
            .unwrap()
            .with_join_str("user");

        let locator_str = locator_path
            .fs_read_text()?;
        let locator
            = Locator::from_file_string(&locator_str)?;

        let diff
            = git::diff_folders(&original_path, &user_path).await?;

        if !self.save {
            let mut stdout = std::io::stdout();
            stdout.write_all(diff.as_bytes()).unwrap();
            stdout.flush().unwrap();
            return Ok(());
        }

        let patch_rel_path
            = Path::try_from(format!(".yarn/patches/{}.patch", locator.slug()))?;

        let patch_descriptor
            = Self::make_patch_descriptor(&locator, &patch_rel_path);

        project
            .import_install_state()?;

        let install_state = project.install_state.as_ref()
            .ok_or(Error::InstallStateNotFound)?;

        if !install_state.normalized_resolutions.contains_key(&locator) {
            return Err(Error::ConflictingOptions(format!("No package found in the project for the given locator: {}", locator.to_print_string())));
        }

        // Find all workspaces that depend on this package and transitive dependencies
        let mut workspace_dependents
            = BTreeMap::new();
        let mut transitive_dependencies
            = BTreeMap::new();

        for (pkg_locator, resolution) in &install_state.normalized_resolutions {
            if pkg_locator.reference.is_virtual_reference() {
                continue;
            }

            let depends_on_the_patched_locator
                = resolution.dependencies.values()
                    .map(|d| Self::ensure_unpatched_descriptor(d))
                    .map(|d| install_state.descriptor_to_locator.get(&d).expect("Dependency not found in descriptor to locator map"))
                    .contains(&locator);

            if !depends_on_the_patched_locator {
                continue;
            }

            if let Some(workspace) = project.try_workspace_by_locator(pkg_locator)? {
                let manifest_content
                    = workspace.manifest_path()
                        .fs_read_prealloc()?;

                let mut formatter
                    = JsonDocument::new(manifest_content)?;

                for (dep_ident, dependency, dependency_type) in workspace.manifest.iter_hard_dependencies() {
                    let unpatchified_descriptor
                        = Self::ensure_unpatched_descriptor(dependency);

                    let dependency_locator
                        = install_state.descriptor_to_locator.get(&unpatchified_descriptor)
                            .expect("Dependency not found in descriptor to locator map");

                    if dependency_locator == &locator {
                        formatter.set_path(
                            &zpm_parsers::Path::from_segments(vec![
                                dependency_type.to_str().to_string(),
                                dep_ident.to_file_string(),
                            ]),
                            Value::String(patch_descriptor.range.to_file_string()),
                        )?;
                    }
                }
            } else {
                for (dep_ident, descriptor) in &resolution.dependencies {
                    if dep_ident != &locator.ident {
                        continue;
                    }

                    let unpatchified_descriptor
                        = Self::ensure_unpatched_descriptor(descriptor);

                    if let Some(dep_locator) = install_state.descriptor_to_locator.get(&unpatchified_descriptor) {
                        if dep_locator == &locator {
                            transitive_dependencies.insert(
                                unpatchified_descriptor.clone(),
                                descriptor.clone(),
                            );
                        }
                    }
                }
            }
        }

        let patch_folder = project.project_cwd
            .with_join(&patch_rel_path)
            .fs_create_dir_all()?;

        let patch_path = patch_folder
            .with_join_str(&format!("{}.patch", locator.slug()));

        patch_path
            .fs_write(&diff)?;

        for (workspace_locator, (dep_ident, original_descriptor)) in &workspace_dependents {
            let workspace
                = project.workspace_by_rel_path(workspace_locator)?;

            let manifest_content
                = workspace.manifest_path()
                    .fs_read_prealloc()?;

            let mut formatter
                = JsonDocument::new(manifest_content)?;

            let dep_type = if workspace.manifest.remote.dependencies.contains_key(dep_ident) {
                "dependencies"
            } else if workspace.manifest.dev_dependencies.contains_key(dep_ident) {
                "devDependencies"
            } else if workspace.manifest.remote.optional_dependencies.contains_key(dep_ident) {
                "optionalDependencies"
            } else {
                continue;
            };

            // Convert the locator to a descriptor to use as the source for the patch
            let source_range = Range::from_file_string(&locator.reference.to_file_string())
                .map_err(|e| Error::InvalidRange(e.to_string()))?;
            let source_descriptor = Descriptor::new(locator.ident.clone(), source_range);
            let new_descriptor = Self::make_patch_descriptor(&source_descriptor, &patch_rel_path);

            formatter.set_path(
                &zpm_parsers::Path::from_segments(vec![
                    dep_type.to_string(),
                    dep_ident.to_file_string(),
                ]),
                Value::String(new_descriptor.range.to_file_string()),
            )?;

            manifest_path.fs_change(&formatter.input, false)?;
        }

        // Add resolutions for transitive dependencies
        if !transitive_dependencies.is_empty() {
            let root_workspace = &project.workspaces[0];
            let manifest_path = root_workspace.path.with_join_str(project::MANIFEST_NAME);
            let manifest_content = manifest_path.fs_read_prealloc()?;
            let mut formatter = JsonDocument::new(manifest_content)?;

            // Convert the locator to a descriptor to use as the source for the patch
            let source_range = Range::from_file_string(&locator.reference.to_file_string())
                .map_err(|e| Error::InvalidRange(e.to_string()))?;
            let source_descriptor = Descriptor::new(locator.ident.clone(), source_range);

            for (original_descriptor, _) in &transitive_dependencies {
                let new_descriptor = Self::make_patch_descriptor(&source_descriptor, &patch_rel_path);

                formatter.set_path(
                    &zpm_parsers::Path::from_segments(vec![
                        "resolutions".to_string(),
                        original_descriptor.to_file_string(),
                    ]),
                    Value::String(new_descriptor.range.to_file_string()),
                )?;
            }

            manifest_path.fs_change(&formatter.input, false)?;
        }

        Ok(())
    }

    fn ensure_unpatched_descriptor(descriptor: &Descriptor) -> Descriptor {
        if let Range::Patch(params) = &descriptor.range {
            params.inner.0.clone()
        } else {
            descriptor.clone()
        }
    }

    fn make_patch_descriptor(source_descriptor: &Descriptor, patch_path: &str) -> Descriptor {
        // Create the patch range using the descriptor protocol
        let patch_range = zpm_primitives::PatchRange {
            inner: Box::new(UrlEncoded(source_descriptor.clone())),
            path: patch_path.to_string(),
        };

        Descriptor::new(
            source_descriptor.ident.clone(),
            Range::Patch(patch_range),
        )
    }
}
