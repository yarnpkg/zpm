use clipanion::cli;
use zpm_parsers::{document::Document, JsonDocument, Value};
use zpm_primitives::Ident;
use zpm_utils::{Path, ToFileString};

use crate::{
    error::Error,
    project::{Project, Workspace, MANIFEST_NAME},
};

/// Link the current project with another one
///
/// This command will add a resolutions entry in the current project manifest (package.json
/// at the top-level workspace), linking it to a remote workspace.
///
/// This is useful when developing packages that depend on each other: you can link them
/// together to test your changes without having to publish them first.
///
/// If the `--all` flag is set, Yarn will link all workspaces from the target project
/// to the current one. By default, private workspaces will be skipped, but this can
/// be toggled using the `--private` flag.
///
/// If the `--relative` flag is set, the paths will be stored as relative paths rather
/// than absolute paths.
#[cli::command]
#[cli::path("link")]
#[cli::category("Dependency management")]
pub struct Link {
    /// Link all workspaces from the target project to the current one
    #[cli::option("-A,--all", default = false)]
    all: bool,

    /// Also link private workspaces
    #[cli::option("-p,--private", default = false)]
    private: bool,

    /// Use relative paths instead of absolute paths
    #[cli::option("-r,--relative", default = false)]
    relative: bool,

    /// The path(s) to the project(s) to link
    destinations: Vec<Path>,
}

impl Link {
    pub async fn execute(&self) -> Result<(), Error> {
        let project = Project::new(None).await?;

        let root_workspace
            = project.root_workspace();
        let root_path
            = &root_workspace.path;

        let manifest_path
            = root_path.with_join_str(MANIFEST_NAME);
        let manifest_content
            = manifest_path.fs_read_prealloc()?;
        let mut document
            = JsonDocument::new(manifest_content)?;

        for destination in &self.destinations {
            let canonical_destination
                = destination.fs_canonicalize()?;

            // Prevent linking a project to itself
            if root_path.contains(&canonical_destination) || canonical_destination.contains(root_path) {
                return Err(Error::CannotLinkToSelf);
            }

            let target_workspace
                = Workspace::from_root_path(&canonical_destination)?;

            if self.all {
                let child_workspaces
                    = target_workspace.workspaces().await?;

                if let Some(name) = &target_workspace.manifest.name {
                    if self.private || !target_workspace.manifest.private.unwrap_or(false) {
                        self.add_resolution(&mut document, name, &canonical_destination, root_path)?;
                    }
                }

                for workspace in child_workspaces {
                    if !self.private && workspace.manifest.private.unwrap_or(false) {
                        continue;
                    }

                    let Some(_) = &workspace.manifest.name else {
                        continue;
                    };

                    self.add_resolution(&mut document, &workspace.name, &workspace.path, root_path)?;
                }
            } else {
                let name
                    = target_workspace.manifest.name.as_ref()
                        .ok_or_else(|| Error::LinkedPackageMissingName(canonical_destination.clone()))?;

                self.add_resolution(&mut document, name, &canonical_destination, root_path)?;
            }
        }

        manifest_path.fs_change(&document.input, false)?;

        Ok(())
    }

    fn add_resolution(&self, document: &mut JsonDocument, name: &Ident, workspace_path: &Path, root_path: &Path) -> Result<(), Error> {
        let portal_path = if self.relative {
            workspace_path.relative_to(root_path)
        } else {
            workspace_path.clone()
        };

        let portal_url = format!("portal:{}", portal_path.to_file_string());

        document.set_path(
            &zpm_parsers::Path::from_segments(vec!["resolutions".to_string(), name.to_file_string()]),
            Value::String(portal_url),
        )?;

        Ok(())
    }
}
