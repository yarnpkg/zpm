use structs::{ConstraintsDependency, ConstraintsPackage, ConstraintsWorkspace};
use zpm_parsers::JsonDocument;
use zpm_primitives::Reference;
use zpm_utils::{Path, ToFileString};

use crate::{
    constraints::structs::{ConstraintsContext, ConstraintsOutput}, error::Error, install::InstallState, project::{Project, Workspace}, resolvers::Resolution, script::ScriptEnvironment
};

pub mod apply;
pub mod structs;

pub async fn check_constraints(project: &Project, fix: bool) -> Result<ConstraintsOutput, Error> {
    let install_state
        = project.install_state.as_ref()
            .ok_or(Error::InstallStateNotFound)?;

    let constraints_workspaces
        = project.workspaces.iter()
            .map(|workspace| to_constraints_workspace(workspace, install_state))
            .collect::<Result<Vec<_>, _>>()?;

    let constraints_packages
        = install_state.resolution_tree.locator_resolutions.iter()
            .map(|(_, resolution)| to_constraints_package(&project, install_state, resolution))
            .collect::<Vec<_>>();

    let constraints_context = ConstraintsContext {
        workspaces: constraints_workspaces,
        packages: constraints_packages,
    };

    let config_path = project.project_cwd
        .with_join_str("yarn.config.cjs");

    let script
        = generate_constraints_adapter(&config_path, &constraints_context, fix);

    let temp_dir
        = Path::temp_dir()?;

    let script_path = temp_dir
        .with_join_str("script.js");
    let result_path = temp_dir
        .with_join_str("result.json");

    script_path
        .fs_write_text(&script)?;

    ScriptEnvironment::new()?
        .with_cwd(project.project_cwd.clone())
        .with_project(&project)
        .enable_shell_forwarding()
        .run_exec("node", &vec![script_path.to_file_string(), result_path.to_file_string()])
        .await?
        .ok()?;

    let result_content = result_path
        .fs_read_prealloc()?;

    let mut output
        = JsonDocument::hydrate_from_slice::<ConstraintsOutput>(&result_content)?;

    output.raw_json = result_content;

    Ok(output)
}

fn generate_constraints_adapter(config_path: &Path, context: &ConstraintsContext, fix: bool) -> String {
    vec![
        "\"use strict\";\n",
        "\n",
        "const CONFIG_PATH =\n",
        &JsonDocument::to_string(&config_path).unwrap(), ";\n",
        "const SERIALIZED_CONTEXT =\n",
        &JsonDocument::to_string(&JsonDocument::to_string(&context).unwrap()).unwrap(), ";\n",
        &format!("const FIX = {};\n", fix),
        "\n",
        std::include_str!("constraints.tpl.js"),
    ].join("")
}

fn to_constraints_workspace<'a>(workspace: &'a Workspace, install_state: &'a InstallState) -> Result<ConstraintsWorkspace, Error> {
    let workspace_resolution = install_state.resolution_tree.locator_resolutions
        .get(&workspace.locator())
        .expect("Workspace not found in resolution tree");

    let dependencies = workspace.manifest.remote.dependencies.iter()
        .map(|(ident, raw_descriptor)| {
            let resolution = if workspace.manifest.dev_dependencies.contains_key(ident) {
                None
            } else {
                let descriptor = workspace_resolution.dependencies.get(ident)
                    .expect("Dependency not found in resolution tree");

                let locator = install_state.resolution_tree.descriptor_to_locator.get(descriptor)
                    .expect("Dependency not found in resolution tree");

                Some(locator.clone())
            };

            ConstraintsDependency {
                ident: ident.clone(),
                range: raw_descriptor.range.clone(),
                dependency_type: "dependencies".to_string(),
                resolution,
            }
        }).collect::<Vec<_>>();

    let peer_dependencies = workspace.manifest.remote.peer_dependencies.iter()
        .map(|(ident, range)| ConstraintsDependency {
            ident: ident.clone(),
            range: range.to_range(),
            dependency_type: "peerDependencies".to_string(),
            resolution: None,
        }).collect::<Vec<_>>();

    let dev_dependencies = workspace.manifest.dev_dependencies.iter()
        .map(|(ident, raw_descriptor)| {
            let descriptor = workspace_resolution.dependencies.get(ident)
                .expect("Dependency not found in resolution tree");

            let locator = install_state.resolution_tree.descriptor_to_locator.get(descriptor)
                .expect("Dependency not found in resolution tree");

            ConstraintsDependency {
                ident: ident.clone(),
                range: raw_descriptor.range.clone(),
                dependency_type: "devDependencies".to_string(),
                resolution: Some(locator.clone()),
            }
        }).collect::<Vec<_>>();

    Ok(ConstraintsWorkspace {
        cwd: workspace.rel_path.clone(),
        ident: workspace.name.clone(),
        dependencies,
        peer_dependencies,
        dev_dependencies,
    })
}

fn to_constraints_package<'a>(project: &'a Project, install_state: &'a InstallState, resolution: &'a Resolution) -> ConstraintsPackage<'a> {
    let dependencies = resolution.dependencies.iter()
        .map(|(ident, descriptor)| {
            (ident, install_state.resolution_tree.descriptor_to_locator.get(descriptor).unwrap())
        }).collect::<Vec<_>>();

    let workspace = if let Reference::WorkspaceIdent(params) = &resolution.locator.reference {
        Some(project.workspace_by_ident(&params.ident).unwrap().rel_path.clone())
    } else {
        None
    };

    ConstraintsPackage {
        locator: resolution.locator.clone(),
        workspace,
        ident: resolution.locator.ident.clone(),
        version: resolution.version.clone(),
        dependencies,
        peer_dependencies: vec![],
    }
}
