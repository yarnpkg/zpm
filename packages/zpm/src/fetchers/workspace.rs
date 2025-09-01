use zpm_primitives::{Locator, WorkspaceIdentReference, WorkspacePathReference};

use crate::{
    error::Error,
    install::{FetchResult, InstallContext},
};

use super::PackageData;

pub fn fetch_locator_ident(context: &InstallContext, _locator: &Locator, params: &WorkspaceIdentReference) -> Result<FetchResult, Error> {
    let project = context.project
        .expect("The project is required for fetching a workspace package");

    let workspace
        = project.workspace_by_ident(&params.ident)?;

    Ok(FetchResult::new(PackageData::Local {
        package_directory: workspace.path.clone(),
        is_synthetic_package: false,
    }))
}

pub fn fetch_locator_path(context: &InstallContext, _locator: &Locator, params: &WorkspacePathReference) -> Result<FetchResult, Error> {
    let project = context.project
        .expect("The project is required for fetching a workspace package");

    let workspace
        = project.workspace_by_rel_path(&params.path)?;

    Ok(FetchResult::new(PackageData::Local {
        package_directory: workspace.path.clone(),
        is_synthetic_package: false,
    }))
}
