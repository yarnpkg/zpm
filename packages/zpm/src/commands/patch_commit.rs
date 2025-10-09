use std::{collections::BTreeMap, io::Write};

use clipanion::cli;
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

        // Save the patch file to the patch folder
        let patch_folder = project.project_cwd.with_join_str(".yarn/patches");
        patch_folder.fs_create_dir_all()?;

        let patch_filename = format!("{}.patch", locator.slug());
        let patch_path = patch_folder.with_join_str(&patch_filename);
        
        patch_path.fs_write(&diff)?;

        // Calculate the patch reference path relative to the project root
        let patch_rel_path = format!("~/{}", patch_path.strip_prefix(&project.project_cwd).unwrap().as_str());

        // Import install state to access stored packages
        project.import_install_state()?;

        let install_state = project.install_state.as_ref()
            .ok_or(Error::InstallStateNotFound)?;

        // Check if the locator exists in the project
        if !install_state.normalized_resolutions.contains_key(&locator) {
            return Err(Error::ConflictingOptions(format!("No package found in the project for the given locator: {}", locator.to_print_string())));
        }

        // Find all workspaces that depend on this package and transitive dependencies
        let mut workspace_dependents = BTreeMap::new();
        let mut transitive_dependencies = BTreeMap::new();

        // Iterate through all packages in the lockfile/resolution
        for (pkg_locator, resolution) in &install_state.normalized_resolutions {
            // Skip virtual locators
            if matches!(pkg_locator.reference, Reference::Virtual(_)) {
                continue;
            }

            // Check if this is a workspace
            let workspace = project.workspaces.iter()
                .find(|ws| {
                    let ws_locator = ws.locator();
                    ws_locator.ident == pkg_locator.ident && 
                    matches!(pkg_locator.reference, Reference::WorkspaceIdent(_) | Reference::WorkspacePath(_))
                });

            if let Some(workspace) = workspace {
                // Check if this workspace depends on the locator we're patching
                for (dep_ident, descriptor) in workspace.manifest.iter_hard_dependencies() {
                    if dep_ident != &locator.ident {
                        continue;
                    }

                    // Get the unpatchified descriptor
                    let unpatchified_descriptor = Self::ensure_unpatched_descriptor(descriptor);

                    // Check if this dependency resolves to our locator
                    if let Some(dep_descriptor) = install_state.descriptor_to_locator.get(&unpatchified_descriptor) {
                        if dep_descriptor == &locator {
                            workspace_dependents.insert(workspace.locator(), (dep_ident.clone(), descriptor.clone()));
                        }
                    }
                }
            } else {
                // This is not a workspace, check if it depends on our target
                // We look in the resolution's dependencies
                for (dep_ident, descriptor) in &resolution.dependencies {
                    if dep_ident != &locator.ident {
                        continue;
                    }

                    // Get the unpatchified descriptor
                    let unpatchified_descriptor = Self::ensure_unpatched_descriptor(descriptor);

                    // Check if this dependency resolves to our locator
                    if let Some(dep_locator) = install_state.descriptor_to_locator.get(&unpatchified_descriptor) {
                        if dep_locator == &locator {
                            transitive_dependencies.insert(unpatchified_descriptor.clone(), descriptor.clone());
                        }
                    }
                }
            }
        }

        // Update workspace manifests
        for (workspace_locator, (dep_ident, original_descriptor)) in &workspace_dependents {
            let workspace = project.workspaces.iter()
                .find(|ws| ws.locator() == *workspace_locator)
                .ok_or_else(|| Error::ConflictingOptions(format!("Workspace not found for locator: {}", workspace_locator.to_print_string())))?;

            let manifest_path = workspace.path.with_join_str(project::MANIFEST_NAME);
            let manifest_content = manifest_path.fs_read_prealloc()?;
            let mut formatter = JsonDocument::new(manifest_content)?;

            // Find which dependency type this is
            let dep_type = if workspace.manifest.remote.dependencies.contains_key(dep_ident) {
                "dependencies"
            } else if workspace.manifest.dev_dependencies.contains_key(dep_ident) {
                "devDependencies"
            } else if workspace.manifest.remote.optional_dependencies.contains_key(dep_ident) {
                "optionalDependencies"
            } else {
                continue;
            };

            let new_descriptor = Self::make_patch_descriptor(original_descriptor, &locator, &patch_rel_path);
            
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

            for (original_descriptor, _) in &transitive_dependencies {
                let new_descriptor = Self::make_patch_descriptor(original_descriptor, &locator, &patch_rel_path);
                
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

    fn make_patch_descriptor(original_descriptor: &Descriptor, _source_locator: &Locator, patch_path: &str) -> Descriptor {
        // Create the patch range using the descriptor protocol
        let patch_range = zpm_primitives::PatchRange {
            inner: Box::new(UrlEncoded(original_descriptor.clone())),
            path: patch_path.to_string(),
        };

        Descriptor::new(
            original_descriptor.ident.clone(),
            Range::Patch(patch_range),
        )
    }
}
