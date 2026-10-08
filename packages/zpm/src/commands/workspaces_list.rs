use clipanion::cli;
use zpm_parsers::JsonDocument;
use zpm_primitives::Ident;
use zpm_utils::{Path, ToFileString};

use crate::{
    error::Error,
    git_utils,
    lockfile_tree::compute_workspace_tree_hashes,
    project::{Project, Workspace},
};

/// List the workspaces in the project
///
/// This command prints the list of workspaces in the project.
///
/// - If `--since` is set, Yarn only lists workspaces modified since the specified ref. Without an explicit ref, Yarn uses the
///   `changesetBaseRefs` configuration (or the `YARN_CHANGESET_BASE` / `TURBO_SCM_BASE` environment variables). Changes to the
///   lockfile mark the workspaces whose dependency tree changed, and changes to files matching `changesetGlobalFiles` mark every
///   workspace.
///
/// - By default the working tree (including uncommitted and untracked files) is compared to the base ref; `--head <ref>` (or the
///   `YARN_CHANGESET_HEAD` / `TURBO_SCM_HEAD` environment variables) compares two refs instead.
///
/// - If `-R,--recursive` is set along with `--since`, Yarn will also list workspaces that depend on workspaces that have been changed since the
///   specified ref, recursively following `dependencies`, `devDependencies`, and `peerDependencies` fields. This is the equivalent of
///   `turbo ls --affected`.
///
/// - If `--no-private` is set, Yarn omits workspaces whose manifest has `private: true`.
///
/// If both `-v,--verbose` and `--json` are set, Yarn also returns cross-dependencies between workspaces (useful when you
/// wish to automatically generate Bazel rules).
///
#[cli::command]
#[cli::path("workspaces", "list")]
#[cli::category("Workspace commands")]
pub struct WorkspacesList {
    /// Also return the cross-dependencies between workspaces
    #[cli::option("-v,--verbose", default = false)]
    verbose: bool,

    /// Also list private workspaces
    #[cli::option("--private", default = true)]
    private: bool,

    /// Only include workspaces that have been changed since the specified ref
    #[cli::option("--since")]
    since: Option<Option<String>>,

    /// Include dependents of changed workspaces when used with `--since`
    #[cli::option("-R,--recursive", default = false)]
    recursive: bool,

    /// Compare against this ref rather than the working tree when used with `--since`
    #[cli::option("--head")]
    head: Option<String>,

    /// Format the output as an NDJSON stream
    #[cli::option("--json", default = false)]
    json: bool,

    /// Include a hash of each workspace dependency tree (requires `--json`)
    #[cli::option("--tree-hash", default = false)]
    tree_hash: bool,
}

impl WorkspacesList {
    fn get_all_list<'a>(&self, project: &'a Project) -> Vec<&'a Workspace> {
        let workspaces
            = project.workspaces.iter()
                .collect();

        workspaces
    }

    async fn get_since_list<'a>(&self, project: &'a Project, since: Option<&str>) -> Result<Vec<&'a Workspace>, Error> {
        let range
            = git_utils::ChangesetRange {
                base: since.map(|since| since.to_string()),
                head: self.head.clone(),
            }.with_env_defaults();

        let workspace_set = match self.recursive {
            true => git_utils::fetch_affected_workspaces(project, &range).await?,
            false => git_utils::fetch_changed_workspaces_in_range(project, &range).await?,
        };

        // We traverse the workspaces in order to ensure that the
        // workspaces are sorted by their position in the workspace list.
        let workspaces
            = project.workspaces.iter()
                .filter(|w| workspace_set.contains(&w.name))
                .collect();

        Ok(workspaces)
    }

    pub async fn execute(&self) -> Result<(), Error> {
        let project
            = Project::new(None).await?;

        let workspaces = match &self.since {
            Some(since) => {
                self.get_since_list(&project, since.as_deref()).await?
            },

            None => {
                self.get_all_list(&project)
            }
        };

        // Without a lockfile there's no dependency tree to describe, and a
        // lockfile we can't read is worth reporting rather than silently
        // turning into a set of hashes computed from nothing.
        let tree_hashes = match self.json && self.tree_hash && project.lockfile_path().fs_exists() {
            true => Some(compute_workspace_tree_hashes(&project, &project.lockfile()?)),
            false => None,
        };

        for workspace in workspaces {
            if workspace.manifest.private == Some(true) && !self.private {
                continue;
            }

            let workspace_path_str
                = workspace.rel_path.to_file_string();

            let workspace_printed_path = match workspace_path_str.is_empty() {
                true => ".",
                false => workspace_path_str.as_str(),
            };

            if self.json {
                #[derive(serde::Serialize)]
                #[serde(rename_all = "camelCase")]
                struct Payload<'a> {
                    location: &'a str,
                    name: Option<&'a Ident>,

                    #[serde(skip_serializing_if = "Option::is_none")]
                    workspace_dependencies: Option<Vec<&'a Path>>,

                    #[serde(skip_serializing_if = "Option::is_none")]
                    mismatched_workspace_dependencies: Option<Vec<String>>,

                    #[serde(skip_serializing_if = "Option::is_none")]
                    tree_hash: Option<String>,
                }

                let mut workspace_dependencies = None;
                let mut mismatched_workspace_dependencies = None;

                if self.verbose {
                    let mut matched_paths = Vec::new();
                    let mut mismatched_strs = Vec::new();

                    for hard in workspace.manifest.iter_hard_dependencies() {
                        let ident = hard.ident;
                        let descriptor = hard.descriptor;
                        let Ok(target_workspace) = project.workspace_by_ident(ident) else {
                            continue;
                        };

                        let target_version = target_workspace.manifest.remote.version.as_ref();

                        let matches = match &descriptor.range {
                            zpm_primitives::Range::WorkspaceMagic(_) => true,
                            zpm_primitives::Range::WorkspacePath(_) => true,
                            // `workspace:my-pkg` re-references the
                            // workspace by ident; since we already
                            // matched the target workspace by ident
                            // above, this always pairs them up.
                            zpm_primitives::Range::WorkspaceIdent(_) => true,
                            zpm_primitives::Range::WorkspaceSemver(params) => {
                                target_version.map_or(true, |v| params.range.check(v))
                            },
                            zpm_primitives::Range::AnonymousSemver(params) => {
                                target_version.map_or(true, |v| params.range.check(v))
                            },
                            zpm_primitives::Range::RegistrySemver(params) => {
                                target_version.map_or(true, |v| params.range.check(v))
                            },
                            _ => false,
                        };

                        if matches {
                            matched_paths.push(&target_workspace.rel_path);
                        } else {
                            mismatched_strs.push(format!("{}@{}", ident.to_file_string(), descriptor.range.to_file_string()));
                        }
                    }

                    workspace_dependencies = Some(matched_paths);
                    mismatched_workspace_dependencies = Some(mismatched_strs);
                }

                let tree_hash = tree_hashes.as_ref()
                    .and_then(|tree_hashes| tree_hashes.get(&workspace.name))
                    .map(|hash| hash.to_file_string());

                let payload = Payload {
                    location: workspace_printed_path,
                    name: workspace.manifest.name.as_ref(),
                    workspace_dependencies,
                    mismatched_workspace_dependencies,
                    tree_hash,
                };

                println!("{}", JsonDocument::to_string(&payload)?);
            } else {
                println!("{}", workspace_printed_path);
            }
        }

        Ok(())
    }
}
