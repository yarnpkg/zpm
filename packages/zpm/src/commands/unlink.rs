use clipanion::cli;
use zpm_macro_enum::zpm_enum;
use zpm_parsers::{document::Document, JsonDocument, Value};
use zpm_primitives::IdentGlob;
use zpm_utils::{ExplicitPath, ToFileString};

use crate::{
    error::Error,
    project::{Project, Workspace, MANIFEST_NAME},
};

#[zpm_enum]
#[derive(Debug)]
#[derive_variants(Debug)]
pub enum UnlinkTarget {
    #[pattern(r"(?<path>.*)")]
    Path {
        path: ExplicitPath,
    },

    #[pattern(r"(?<pattern>.*)")]
    Glob {
        pattern: IdentGlob,
    },
}

/// Unlink the current project from a previously linked one
///
/// This command will remove any resolutions in the project-level manifest that
/// would have been added via a `yarn link` with similar arguments.
///
/// If the `--all` flag is used without any arguments, Yarn will remove all
/// resolutions that use the `portal:` protocol.
///
/// If the `--all` flag is used with a path argument, Yarn will unlink all
/// workspaces from the specified project.
///
/// Otherwise, you can specify package names (with glob support) or paths to
/// individual packages to unlink specific resolutions.
#[cli::command]
#[cli::path("unlink")]
#[cli::category("Dependency management")]
pub struct Unlink {
    /// Unlink all portal resolutions, or all workspaces from the target project
    #[cli::option("-A,--all", default = false)]
    all: bool,

    /// The path(s) or pattern(s) to unlink
    targets: Vec<UnlinkTarget>,
}

impl Unlink {
    pub async fn execute(&self) -> Result<(), Error> {
        let project
            = Project::new(None).await?;

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

        let mut to_unlink
            = Vec::new();

        if self.targets.is_empty() {
            if self.all {
                for (selector, range) in root_workspace.manifest.resolutions.iter() {
                    if matches!(range, zpm_primitives::Range::Portal(_)) {
                        to_unlink.push(selector.target_ident().clone());
                    }
                }
            }
        } else {
            for target in &self.targets {
                match target {
                    UnlinkTarget::Path(explicit_path) => {
                        let canonical_path
                            = explicit_path.path.raw_path.path.fs_canonicalize()?;

                        if self.all {
                            let target_workspace
                                = Workspace::from_root_path(&canonical_path)?;
                            let child_workspaces
                                = target_workspace.workspaces().await?;

                            // Add root workspace's name if it has one
                            if let Some(name) = &target_workspace.manifest.name {
                                to_unlink.push(name.clone());
                            }

                            for workspace in child_workspaces {
                                if let Some(name) = &workspace.manifest.name {
                                    to_unlink.push(name.clone());
                                }
                            }
                        } else {
                            let target_workspace
                                = Workspace::from_root_path(&canonical_path)?;

                            if let Some(name) = &target_workspace.manifest.name {
                                to_unlink.push(name.clone());
                            }
                        }
                    }
                    UnlinkTarget::Glob(glob) => {
                        for (selector, range) in root_workspace.manifest.resolutions.iter() {
                            if matches!(range, zpm_primitives::Range::Portal(_)) {
                                let ident
                                    = selector.target_ident();

                                if glob.pattern.check(ident) {
                                    to_unlink.push(ident.clone());
                                }
                            }
                        }
                    }
                }
            }
        }

        for ident in &to_unlink {
            document.set_path(
                &zpm_parsers::Path::from_segments(vec!["resolutions".to_string(), ident.to_file_string()]),
                Value::Undefined,
            )?;
        }

        manifest_path.fs_change(&document.input, false)?;

        Ok(())
    }
}
