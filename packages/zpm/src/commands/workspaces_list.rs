use clipanion::cli;
use zpm_utils::Path;

use crate::{error::Error, primitives::{Descriptor, Ident, Reference}, project};

#[cli::command]
#[cli::path("workspaces", "list")]
#[cli::category("Workspace commands")]
#[cli::description("List the workspaces in the project")]
pub struct WorkspacesList {
    #[cli::option("-v,--verbose", default = false)]
    verbose: bool,

    #[cli::option("--private", default = true)]
    private: bool,

    #[cli::option("--json", default = false)]
    json: bool,
}

impl WorkspacesList {
    #[tokio::main()]
    pub async fn execute(&self) -> Result<(), Error> {
        let mut project
            = project::Project::new(None).await?;

        if self.verbose {
            project
                .lazy_install().await?;
        }

        for workspace in &project.workspaces {
            if workspace.manifest.private == Some(true) && !self.private {
                continue;
            }

            let workspace_path_str
                = workspace.rel_path.to_string();

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

                println!("{}", sonic_rs::to_string(&payload)?);
            } else {
                println!("{}", workspace_printed_path);
            }
        }

        Ok(())
    }
}
