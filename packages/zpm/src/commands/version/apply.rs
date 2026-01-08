use std::collections::BTreeMap;

use clipanion::cli;
use indexmap::IndexMap;
use zpm_utils::{AbstractValue, tree};

use crate::{error::Error, project, versioning};

/// This command will apply the deferred version changes and remove their definitions from the repository.
///
/// Note that if `--prerelease` is set, the given prerelease identifier (by default `rc.%n`) will be used on all new versions and the version definitions will be kept as-is.
///
/// By default only the current workspace will be bumped, but you can configure this behavior by using one of:
///
/// - `--recursive` to also apply the version bump on its dependencies
/// - `--all` to apply the version bump on all packages in the repository
///
/// Note that this command will also update the `workspace:` references across all your local workspaces, thus ensuring that they keep referring to the same workspaces even after the version bump.
#[cli::command]
#[cli::path("version", "apply")]
#[cli::category("Project management")]
pub struct VersionApply {
    /// Apply the deferred version changes on all workspaces
    #[cli::option("--all", default = false)]
    all: bool,

    /// Print the versions without actually generating the package archive
    #[cli::option("--dry-run", default = false)]
    dry_run: bool,

    /// Add a prerelease identifier to new versions
    #[cli::option("--prerelease")]
    prerelease: Option<String>,

    /// Use the exact version of each package, removes any range. Useful for nightly releases where the range might match another version.
    #[cli::option("--exact", default = false)]
    exact: bool,

    /// Release the transitive workspaces as well
    #[cli::option("-R,--recursive", default = false)]
    recursive: bool,

    /// Format the output as an NDJSON stream
    #[cli::option("--json", default = false)]
    json: bool,
}

impl VersionApply {
    pub async fn execute(&self) -> Result<(), Error> {
        let project
            = project::Project::new(None).await?;

        let active_workspace
            = project.active_workspace()?;

        let versioning
            = versioning::Versioning::new(&project);

        let mut releases
            = versioning.resolve_releases()?;

        if self.all {
            if releases.is_empty() {
                return Err(Error::NoVersionBumpRequiredForProject);
            }
        } else {
            if self.recursive {
                unimplemented!()
            } else {
                let Some(requested_version) = releases.remove(&active_workspace.name) else {
                    return Err(if releases.is_empty() {
                        Error::NoVersionFoundForActiveWorkspace
                    } else {
                        Error::NoVersionBumpRequiredForActiveWorkspaceSuggestAll
                    });
                };

                releases = BTreeMap::from_iter([(
                    active_workspace.name.clone(),
                    requested_version,
                )]);
            }
        }

        let mut root_children
            = Vec::new();

        for (workspace_ident, next_version) in releases.iter() {
            let workspace
                = project.workspace_by_ident(workspace_ident)?;

            let mut workspace_children
                = IndexMap::new();

            let current_version
                = workspace.manifest.remote.version.as_ref()
                    .ok_or(Error::NoVersionFoundForWorkspace(workspace_ident.clone()))?;

            workspace_children.insert("current".to_string(), tree::Node {
                label: Some("Current version".to_string()),
                value: Some(AbstractValue::new(current_version.clone())),
                children: None,
            });

            workspace_children.insert("next".to_string(), tree::Node {
                label: Some("Next version".to_string()),
                value: Some(AbstractValue::new(next_version.clone())),
                children: None,
            });

            root_children.push(tree::Node {
                label: None,
                value: Some(AbstractValue::new(workspace_ident.clone())),
                children: Some(tree::TreeNodeChildren::Map(workspace_children)),
            });
        }

        let root_node = tree::Node {
            label: None,
            value: None,
            children: Some(tree::TreeNodeChildren::Vec(vec![])),
        };

        let rendering
            = tree::TreeRenderer::new()
                .render(&root_node, self.json);

        print!("{}", rendering);

        if self.dry_run {
            return Ok(());
        }

        for (workspace_ident, version) in releases.iter() {
            versioning.set_immediate_version(workspace_ident, version)?;
        }

        Ok(())
    }
}
