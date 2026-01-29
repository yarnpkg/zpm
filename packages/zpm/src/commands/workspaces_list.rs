use std::{collections::{BTreeMap, BTreeSet}, str::FromStr};

use clipanion::cli;
use zpm_parsers::JsonDocument;
use zpm_primitives::{Ident, Reference};
use zpm_utils::{Path, ToFileString};

use crate::{
    error::Error,
    git_utils,
    project::{Project, Workspace},
};

/// List the workspaces in the project
///
/// This command will print the list of all workspaces in the project.
///
/// - If `--since` is set, Yarn will only list workspaces that have been modified since the specified ref. By default Yarn will use the refs
///   specified by the `changesetBaseRefs configuration option.
///
/// - If `-R,--recursive` is set along with `--since`, Yarn will also list workspaces that depend on workspaces that have been changed since the
///   specified ref, recursively following `dependencies` and `devDependencies` fields.
///
/// - If `--no-private` is set, Yarn will not list any workspaces that have the `private` field set to true.
///
/// If both the `-v,--verbose` and `--json` options are set, Yarn will also return the cross-dependencies between each workspaces (useful when you
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

    /// Follow dependencies
    #[cli::option("-R,--recursive", default = false)]
    recursive: bool,

    /// Format the output as an NDJSON stream
    #[cli::option("--json", default = false)]
    json: bool,
}

impl WorkspacesList {
    fn get_all_list<'a>(&self, project: &'a Project) -> Vec<&'a Workspace> {
        let workspaces
            = project.workspaces.iter()
                .collect();

        workspaces
    }

    async fn get_since_list<'a>(&self, project: &'a Project, since: Option<&str>) -> Result<Vec<&'a Workspace>, Error> {
        let changed_files
            = git_utils::fetch_changed_files(project, since).await?;

        let mut workspace_set
            = BTreeSet::new();

        let ignored_files = BTreeSet::from_iter([
            Path::from_str("yarn.lock").unwrap(),
            Path::from_str(".pnp.cjs").unwrap(),
            Path::from_str(".pnp.loader.mjs").unwrap(),
        ]);

        let ignored_paths = [
            Path::from_str(".yarn").unwrap(),
        ];

        for p in changed_files {
            let rel_p = p
                .forward_relative_to(&project.project_cwd);

            if let Some(rel_p) = rel_p {
                if ignored_files.contains(&rel_p) || ignored_paths.iter().any(|ignored_path| ignored_path.contains(&rel_p)) {
                    continue;
                }

                let containing_workspace = project.workspaces.iter()
                    .filter(|w| w.rel_path.contains(&rel_p))
                    .max_by_key(|w| w.rel_path.as_str().len());

                if let Some(workspace) = containing_workspace {
                    workspace_set.insert(workspace.name.clone());
                }
            }
        }

        if self.recursive {
            let install_state = project.install_state.as_ref()
                .expect("Expected the install state to have been retrieved earlier");

            let mut dependent_map
                = BTreeMap::new();

            for workspace in project.workspaces.iter() {
                let workspace_resolution = install_state.resolution_tree.locator_resolutions.get(&workspace.locator())
                    .expect("Expected the workspace to be in the resolution tree");

                for dependency_descriptor in workspace_resolution.dependencies.values() {
                    let dependency_locator = install_state.resolution_tree.descriptor_to_locator.get(dependency_descriptor)
                        .expect("Expected the descriptor to be in the resolution tree");

                    let Reference::WorkspaceIdent(locator_params) = &dependency_locator.reference else {
                        continue;
                    };

                    dependent_map.entry(locator_params.ident.clone())
                        .or_insert(BTreeSet::new())
                        .insert(workspace.name.clone());
                }
            }

            let mut queue = workspace_set.iter()
                .cloned()
                .collect::<Vec<_>>();

            while let Some(workspace_ident) = queue.pop() {
                let dependents
                    = dependent_map.get(&workspace_ident);

                if let Some(dependents) = dependents {
                    for dependent in dependents {
                        if workspace_set.insert(dependent.clone()) {
                            queue.push(dependent.clone());
                        }
                    }
                }
            }
        }

        // We traverse the workspaces in order to ensure that the
        // workspaces are sorted by their position in the workspace list.
        let workspaces
            = project.workspaces.iter()
                .filter(|w| workspace_set.contains(&w.name))
                .collect();

        Ok(workspaces)
    }

    pub async fn execute(&self) -> Result<(), Error> {
        let mut project
            = Project::new(None).await?;

        if self.verbose || (self.recursive && self.since.is_some()) {
            project
                .lazy_install().await?;
        }

        let workspaces = match &self.since {
            Some(since) => {
                self.get_since_list(&project, since.as_deref()).await?
            },

            None => {
                self.get_all_list(&project)
            }
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
                    mismatched_workspace_dependencies: Option<Vec<&'a str>>,
                }

                let mut workspace_dependencies = None;
                let mut mismatched_workspace_dependencies = None;

                if self.verbose {
                    let install_state = project.install_state.as_ref()
                        .expect("Expected the install state to have been retrieved earlier");

                    let workspace_resolution = install_state.resolution_tree.locator_resolutions.get(&workspace.locator())
                        .expect("Expected the workspace to be in the resolution tree");

                    // TODO: Deprecate this field; we can't run the command if the workspaces
                    // are mismatched anyway, since the install will fail.
                    mismatched_workspace_dependencies = Some(vec![]);

                    workspace_dependencies = Some(workspace_resolution.dependencies.values()
                        .filter_map(|dependency_descriptor| {
                            let dependency_locator = install_state.resolution_tree.descriptor_to_locator.get(dependency_descriptor)
                                .expect("Expected the descriptor to be in the resolution tree");

                            let Reference::WorkspaceIdent(locator_params) = &dependency_locator.reference else {
                                return None;
                            };

                            let workspace = project
                                .workspace_by_ident(&locator_params.ident)
                                .expect("Expected the workspace to be in the project");

                            Some(&workspace.rel_path)
                        }).collect::<Vec<_>>());
                }

                let payload = Payload {
                    location: workspace_printed_path,
                    name: workspace.manifest.name.as_ref(),
                    workspace_dependencies,
                    mismatched_workspace_dependencies,
                };

                println!("{}", JsonDocument::to_string(&payload)?);
            } else {
                println!("{}", workspace_printed_path);
            }
        }

        Ok(())
    }
}
