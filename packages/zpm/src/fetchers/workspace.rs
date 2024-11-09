use crate::{error::Error, install::{FetchResult, InstallContext}, primitives::{reference, Locator}};

use super::PackageData;

pub fn fetch_locator(context: &InstallContext, _locator: &Locator, params: &reference::WorkspaceReference) -> Result<FetchResult, Error> {
    let project = context.project
        .expect("The project is required for fetching a workspace package");

    let workspace = project.workspaces
        .get(&params.ident)
        .ok_or_else(|| Error::WorkspaceNotFound(params.ident.clone()))?;

    Ok(FetchResult::new(PackageData::Local {
        package_directory: workspace.path.clone(),
        discard_from_lookup: false,
    }))
}
