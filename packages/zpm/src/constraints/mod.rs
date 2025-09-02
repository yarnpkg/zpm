use structs::{ConstraintsDependency, ConstraintsPackage, ConstraintsWorkspace};
use zpm_primitives::Reference;

use crate::{
    error::Error,
    install::InstallState,
    project::{Project, Workspace},
    resolvers::Resolution,
};

pub mod apply;
pub mod structs;

pub fn to_constraints_workspace<'a>(workspace: &'a Workspace, install_state: &'a InstallState) -> Result<ConstraintsWorkspace, Error> {
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

pub fn to_constraints_package<'a>(project: &'a Project, install_state: &'a InstallState, resolution: &'a Resolution) -> ConstraintsPackage<'a> {
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
